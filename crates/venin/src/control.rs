//! `venin status`, `pause`, `resume`, `priors restore` and `launch set-ca`. A paused runner writes each period as
//! void (`paused`) until it is resumed.

use crate::config::Config;
use crate::net::Net;
use crate::priors::{OUT, VERSION};
use crate::store::Store;
use alloy::primitives::Address;
use serde::{Deserialize, Serialize};
use serde_json::json;

fn store(config: &Config) -> Store {
    Store::new(config.data_dir.clone(), config.state_dir.clone())
}

pub fn pause(config: &Config) -> Result<(), String> {
    store(config).write_state_json("paused", &crate::now())?;
    println!("paused: periods from the next start are void (paused) until venin resume");
    Ok(())
}

pub fn resume(config: &Config) -> Result<(), String> {
    store(config).remove_state("paused")?;
    println!("resumed");
    Ok(())
}

/// Brings Priors back into the ward after a seal took it out, from the runner's next period, and logs why.
pub fn restore_priors(config: &Config, reason: &str) -> Result<(), String> {
    let reason = reason.trim();
    if reason.is_empty() {
        return Err("say why Priors comes back: --reason <text>".into());
    }
    let s = store(config);
    let out: serde_json::Value = s
        .state_json(OUT)?
        .ok_or_else(|| format!("Priors {VERSION} is not out of the ward"))?;
    s.remove_state(OUT)?;
    crate::runner::log(&format!(
        "Priors {} is back in the ward (out since period {}): {reason}",
        out["version"].as_str().unwrap_or(VERSION),
        out["n"]
    ));
    Ok(())
}

pub fn status(config: &Config) -> Result<(), String> {
    let s = store(config);
    for rel in ["head.json", "status.json"] {
        match s.read(rel)? {
            Some(b) => println!("{rel}: {}", String::from_utf8_lossy(&b)),
            None => println!("{rel}: none yet"),
        }
    }
    // The address to fund before genesis, and to watch after it.
    match crate::anchor::key_address(&config.anchor.key_file, config.anchor.chain_id) {
        Ok(a) => println!(
            "anchor key: {} on chain {}",
            a.to_checksum(None),
            config.anchor.chain_id
        ),
        Err(e) => println!("anchor key: {e}"),
    }
    println!("paused: {}", s.state_exists("paused"));
    println!(
        "anchors: {} sent, {} due, one in flight: {}",
        s.anchors()?.len(),
        s.due()?.len(),
        s.state_exists("inflight.json")
    );
    Ok(())
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Launch {
    pub ca: String,
    pub chain_id: u64,
    /// The burn per release, in the token's smallest unit, as a decimal string.
    pub amount: String,
    pub set_s: i64,
    /// When burning opens on /release.
    pub opens_s: i64,
}

/// Burning opens this long after the token contract is set at launch.
pub const RELEASE_DELAY_S: i64 = 2 * 3600;

/// An address in its EIP-55 mixed-case form; any other spelling is refused.
pub fn checksummed(address: &str) -> Result<Address, String> {
    Address::parse_checksummed(address, None)
        .map_err(|_| format!("{address} is not an EIP-55 checksummed address"))
}

/// Records the token contract once its address is checksummed, matches `[release] token` and holds code on the
/// token's chain, then writes the release settings (contract, chain, burn amount, opening time) into the web build's
/// env file. The site shows them after its next build.
pub fn set_ca(config: &Config, address: &str) -> Result<(), String> {
    let release = config
        .release
        .as_ref()
        .ok_or("set-ca needs a [release] section with the token and the burn amount")?;
    let ca = checksummed(address)?;
    if !release.token.eq_ignore_ascii_case(&format!("{ca:#x}")) {
        return Err(format!(
            "[release] token is {}; set it to {address} first, so the runner and the site agree",
            release.token
        ));
    }
    let net = Net::new(&release.rpcs)?;
    let code = net.rpc(
        &release.rpcs[0],
        "eth_getCode",
        json!([format!("{ca:#x}"), "latest"]),
    )?;
    let code = code.as_str().unwrap_or("0x");
    if code.len() <= 2 {
        return Err(format!(
            "{address} holds no contract code on chain {}",
            release.chain_id
        ));
    }
    let now = crate::now();
    let launch = Launch {
        ca: ca.to_checksum(None),
        chain_id: release.chain_id,
        amount: release.amount.clone(),
        set_s: now,
        opens_s: now + RELEASE_DELAY_S,
    };
    store(config).write_state_json("launch.json", &launch)?;
    let path = &config.web_env;
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    std::fs::write(path, web_env(&text, &launch))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    println!(
        "token contract {} ({} bytes of code); burning opens at {}; release settings written to {}; rebuild the \
         site to show them",
        launch.ca,
        (code.len() - 2) / 2,
        venin_core::time::rfc3339(launch.opens_s).unwrap_or_default(),
        path.display()
    );
    Ok(())
}

/// The env file `text` with the release keys set from `launch`; every other line is kept.
pub fn web_env(text: &str, launch: &Launch) -> String {
    let keys = [
        ("VITE_RELEASE_CA", launch.ca.clone()),
        ("VITE_RELEASE_CHAIN_ID", launch.chain_id.to_string()),
        ("VITE_RELEASE_AMOUNT", launch.amount.clone()),
        ("VITE_RELEASE_OPENS", launch.opens_s.to_string()),
    ];
    let mut out: String = text
        .lines()
        .filter(|l| {
            let key = l.split('=').next().unwrap_or("").trim();
            !keys.iter().any(|(k, _)| *k == key)
        })
        .map(|l| format!("{l}\n"))
        .collect();
    for (k, v) in keys {
        out.push_str(&format!("{k}={v}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::tests::text;
    use crate::net::tests::serve;

    #[test]
    fn only_checksummed_addresses_pass() {
        let good = "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed";
        assert!(checksummed(good).is_ok());
        assert!(checksummed(&good.to_lowercase()).is_err());
        assert!(checksummed(&good.replace('a', "A")).is_err());
        assert!(checksummed("0x1234").is_err());
    }

    #[test]
    fn set_ca_needs_code_and_pause_toggles() {
        let (url, _) = serve(|_, _, body| {
            let code = if body.contains("5aaeb6053f3e94c9b9a09f33669435e7ef1beaed") {
                "0x6001"
            } else {
                "0x"
            };
            (
                200,
                Vec::new(),
                format!(r#"{{"jsonrpc":"2.0","id":1,"result":"{code}"}}"#),
            )
        });
        let dir = crate::anchor::tests::temp("ca");
        let mut config = Config::parse(&text(&dir, &url, "http://127.0.0.1:2/v1")).expect("config");
        let ca = "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed";
        assert!(set_ca(&config, ca).is_err(), "no [release] section");
        config.release = Some(crate::config::Release {
            queue_url: format!("{url}/api/queue"),
            slots: 6,
            chain_id: 4663,
            token: ca.to_lowercase(),
            amount: "5000".into(),
            rpcs: vec![url.clone()],
            defend_url: String::new(),
            defend_slots: 0,
        });
        config.web_env = dir.join("web.env");
        std::fs::write(
            &config.web_env,
            "VITE_DATA_URL=https://data.invalid\nVITE_RELEASE_CA=0xold\n",
        )
        .expect("writes");
        set_ca(&config, ca).expect("sets");
        let launch: Launch = store(&config)
            .state_json("launch.json")
            .expect("reads")
            .expect("written");
        assert_eq!(launch.ca, ca);
        assert_eq!((launch.chain_id, launch.amount.as_str()), (4663, "5000"));
        assert_eq!(launch.opens_s, launch.set_s + RELEASE_DELAY_S);
        assert_eq!(
            std::fs::read_to_string(&config.web_env).expect("reads"),
            format!(
                "VITE_DATA_URL=https://data.invalid\nVITE_RELEASE_CA={ca}\nVITE_RELEASE_CHAIN_ID=4663\n\
                 VITE_RELEASE_AMOUNT=5000\nVITE_RELEASE_OPENS={}\n",
                launch.opens_s
            )
        );
        let other = "0xfB6916095ca1df60bB79Ce92cE3Ea74c37c5d359";
        assert!(set_ca(&config, other).is_err(), "not the configured token");
        if let Some(r) = config.release.as_mut() {
            r.token = other.into();
        }
        assert!(set_ca(&config, other).is_err(), "no code there");
        pause(&config).expect("pauses");
        assert!(store(&config).state_exists("paused"));
        status(&config).expect("status");
        resume(&config).expect("resumes");
        assert!(!store(&config).state_exists("paused"));
    }
}
