//! A holder's release signature: the message they sign and the address an EIP-191 `personal_sign` signature over it
//! recovers.

use crate::strain::Route;
use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
use sha3::{Digest, Keccak256};

/// The message a holder signs; docs/RECORD.md, "Holder intake".
pub fn message(tx: &str, route: Route, payload_sha: &str, chain_id: u64) -> String {
    format!(
        "Venin strain release\nburn: {tx}\nroute: {route}\npayload-sha256: {payload_sha}\nchain: {chain_id}"
    )
}

/// The message a holder signs to put their agent in the ward; docs/RECORD.md, "Defend intake". `column` is the model's
/// column (`m1` to `m4`).
pub fn defend_message(tx: &str, column: &str, rules_sha: &str, chain_id: u64) -> String {
    format!(
        "Venin defend\nburn: {tx}\nmodel: {column}\nrules-sha256: {rules_sha}\nchain: {chain_id}"
    )
}

fn keccak(bytes: &[u8]) -> [u8; 32] {
    Keccak256::digest(bytes).into()
}

/// The address, as lower-case `0x` hex, whose key made `signature` (65 bytes of hex, `v` 27, 28, 0 or 1) over
/// `message`; `None` when it recovers none.
pub fn recover(message: &str, signature: &str) -> Option<String> {
    let bytes = crate::unhex(signature).filter(|b| b.len() == 65)?;
    let v = match bytes[64] {
        v @ (27 | 28) => v - 27,
        v @ (0 | 1) => v,
        _ => return None,
    };
    let mut prefixed = format!("\u{19}Ethereum Signed Message:\n{}", message.len()).into_bytes();
    prefixed.extend_from_slice(message.as_bytes());
    let signature = Signature::from_slice(&bytes[..64]).ok()?;
    let key = VerifyingKey::recover_from_prehash(
        &keccak(&prefixed),
        &signature,
        RecoveryId::from_byte(v)?,
    )
    .ok()?;
    let point = key.to_encoded_point(false);
    Some(format!(
        "0x{}",
        crate::hex(&keccak(&point.as_bytes()[1..])[12..])
    ))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use k256::ecdsa::SigningKey;

    /// A fixed key: its address and a signature over `message` in the form wallets give.
    pub fn sign(message: &str) -> (String, String) {
        let key = SigningKey::from_slice(&[7; 32]).expect("a key");
        let mut prefixed =
            format!("\u{19}Ethereum Signed Message:\n{}", message.len()).into_bytes();
        prefixed.extend_from_slice(message.as_bytes());
        let (sig, id) = key
            .sign_prehash_recoverable(&keccak(&prefixed))
            .expect("signs");
        let mut bytes = sig.to_bytes().to_vec();
        bytes.push(27 + id.to_byte());
        let point = key.verifying_key().to_encoded_point(false);
        let address = format!("0x{}", crate::hex(&keccak(&point.as_bytes()[1..])[12..]));
        (address, format!("0x{}", crate::hex(&bytes)))
    }

    #[test]
    fn a_signature_recovers_its_signer_and_nothing_else() {
        let text = message("0xab", Route::Skl, "cd", 4663);
        assert_eq!(
            text,
            "Venin strain release\nburn: 0xab\nroute: SKL\npayload-sha256: cd\nchain: 4663"
        );
        let (address, signature) = sign(&text);
        assert_eq!(recover(&text, &signature), Some(address.clone()));
        assert_ne!(
            recover(&message("0xab", Route::Skl, "cd", 1), &signature),
            Some(address)
        );
        assert_eq!(recover(&text, "0x00"), None);
        let mut bad = signature.clone();
        bad.replace_range(bad.len() - 2.., "1d");
        assert_eq!(recover(&text, &bad), None);
    }
}
