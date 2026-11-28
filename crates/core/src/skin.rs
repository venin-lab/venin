//! The skin genes as pure checks, with no world in them: the filters on text a body reads, and the decision on one
//! acting call. `tools` runs them on the ward's fake ledger, and `guard` runs the same code on any agent's tools.

use crate::gene::{Gene, Kind, Rule};
use crate::strain::{fold, is_hidden, reveals};

/// Read filters in the order they run.
pub const READ_FILTERS: [Rule; 4] = [
    Rule::StripHidden,
    Rule::StripComments,
    Rule::QuarantineToolText,
    Rule::Datamark,
];

/// Checks on an acting call in the order they run; genes of one rule run in id order.
pub const CALL_CHECKS: [Rule; 7] = [
    Rule::OneAction,
    Rule::AllowlistPayees,
    Rule::AmountCap,
    Rule::DomainAllowlist,
    Rule::PlanLock,
    Rule::EgressCanary,
    Rule::Datamark,
];

/// What `quarantine_tool_text` puts in place of a skill's text.
pub const WITHHELD_SKILL: &str = "[skill text withheld by policy]";
/// What `quarantine_tool_text` puts in place of a third-party tool's description.
pub const WITHHELD_TOOL: &str = "Third-party tool. Its description was withheld by policy.";

/// What `datamark` puts in place of every space in text the body reads.
pub fn datamark_char() -> char {
    char::from_u32(0x2c6).expect("a valid code point")
}

/// The skin genes of a genome, in id order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Skin {
    genes: Vec<Gene>,
}

impl Skin {
    pub fn new(genes: &[&Gene]) -> Self {
        let mut genes: Vec<Gene> = genes
            .iter()
            .filter(|g| g.kind == Kind::Skin)
            .map(|g| (*g).clone())
            .collect();
        genes.sort_by(|a, b| a.id.cmp(&b.id));
        Skin { genes }
    }

    fn with(&self, rule: Rule) -> impl Iterator<Item = &Gene> {
        self.genes.iter().filter(move |g| g.rule == Some(rule))
    }

    /// The first gene of `rule`, in id order.
    pub fn first(&self, rule: Rule) -> Option<&Gene> {
        self.with(rule).next()
    }
}

/// `text` after the skin's filters among `rules`, and the genes that changed it.
pub fn filter(text: &str, skin: &Skin, rules: &[Rule]) -> (String, Vec<String>) {
    let mut out = text.to_string();
    let mut changed = Vec::new();
    for rule in rules {
        let Some(gene) = skin.first(*rule) else {
            continue;
        };
        let next = match rule {
            Rule::StripHidden => strip_hidden(&out),
            Rule::StripComments => strip_comments(&out),
            Rule::QuarantineToolText => WITHHELD_SKILL.to_string(),
            Rule::Datamark => datamark(&out),
            _ => continue,
        };
        if next != out {
            changed.push(gene.id.clone());
            out = next;
        }
    }
    (out, changed)
}

/// `strip_hidden`: characters that render as nothing dropped, then `<span>` elements styled to be invisible.
pub fn strip_hidden(text: &str) -> String {
    strip_invisible_spans(&text.chars().filter(|c| !is_hidden(*c)).collect::<String>())
}

/// `strip_comments`: every `<!-- ... -->` dropped, and an unclosed one to the end.
pub fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        rest = rest[start + 4..]
            .find("-->")
            .map_or("", |end| &rest[start + 4 + end + 3..]);
    }
    out.push_str(rest);
    out
}

/// `datamark`: every space replaced with `datamark_char`.
pub fn datamark(text: &str) -> String {
    text.replace(' ', &datamark_char().to_string())
}

/// Drops `<span style="...">...</span>` elements styled to be invisible.
fn strip_invisible_spans(text: &str) -> String {
    const INVISIBLE: [&str; 6] = [
        "color:#fff",
        "color:white",
        "font-size:0",
        "font-size:1px",
        "display:none",
        "opacity:0",
    ];
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while let Some(found) = lower[at..].find("<span") {
        let start = at + found;
        let Some(open_end) = lower[start..].find('>').map(|i| start + i + 1) else {
            break;
        };
        let style: String = lower[start..open_end]
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let close = lower[open_end..]
            .find("</span>")
            .map(|i| open_end + i + "</span>".len());
        out.push_str(&text[at..start]);
        match close {
            Some(end) if INVISIBLE.iter().any(|s| style.contains(s)) => at = end,
            _ => {
                out.push_str(&text[start..open_end]);
                at = open_end;
            }
        }
    }
    out.push_str(&text[at..]);
    out
}

/// What the owner of the agent allows, as the call checks read it. Addresses compare after folding, so `book`,
/// `email` and `plan` hold folded text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Allowed {
    /// Payees in the owner's address book.
    pub book: Vec<String>,
    /// The owner's e-mail domain.
    pub domain: String,
    /// The owner's e-mail address.
    pub email: String,
    /// The owner's message for the task.
    pub plan: String,
    /// Values that must never leave in any encoding.
    pub secrets: Vec<String>,
}

/// One acting call: a payment (`funds`) or a message, its trimmed recipient, the amount in cents (0 for a message),
/// and every string argument.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call<'a> {
    pub funds: bool,
    pub to: &'a str,
    pub amount_cents: u64,
    pub strings: Vec<&'a str>,
}

/// The first gene that stops `call` after `actions` earlier actions of the session, or none.
pub fn stops<'s>(
    skin: &'s Skin,
    allowed: &Allowed,
    actions: usize,
    call: &Call,
) -> Option<&'s Gene> {
    let to_folded = fold(call.to);
    let funds = call.funds;
    let in_book = allowed.book.contains(&to_folded);
    for rule in CALL_CHECKS {
        for gene in skin.with(rule) {
            let stops = match rule {
                Rule::OneAction => actions >= 1,
                Rule::AllowlistPayees => funds && !in_book,
                Rule::AmountCap => {
                    funds && gene.cap_cents().is_some_and(|cap| call.amount_cents > cap)
                }
                Rule::DomainAllowlist => {
                    !funds && {
                        let domain = to_folded
                            .rsplit('@')
                            .next()
                            .unwrap_or_default()
                            .trim_end_matches('.');
                        domain != allowed.domain
                            && !gene.domains(&allowed.domain).iter().any(|d| d == domain)
                    }
                }
                Rule::PlanLock => {
                    !allowed.plan.contains(&to_folded) && !in_book && to_folded != allowed.email
                }
                Rule::EgressCanary => call
                    .strings
                    .iter()
                    .any(|s| allowed.secrets.iter().any(|secret| reveals(s, secret))),
                Rule::Datamark => call.strings.iter().any(|s| s.contains(datamark_char())),
                _ => false,
            };
            if stops {
                return Some(gene);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gene::tests::skin;
    use serde_json::json;

    fn allowed() -> Allowed {
        Allowed {
            book: vec!["0xaaaa".into()],
            domain: "acme.test".into(),
            email: "sam@acme.test".into(),
            plan: fold("Pay 0xBBBB the 120 for the hall tap."),
            secrets: vec!["vnk_secret".into()],
        }
    }

    fn call<'a>(funds: bool, to: &'a str, strings: &[&'a str]) -> Call<'a> {
        Call {
            funds,
            to,
            amount_cents: if funds { 12_000 } else { 0 },
            strings: strings.to_vec(),
        }
    }

    fn stopped(rule: Rule, params: serde_json::Value, actions: usize, c: &Call) -> bool {
        let gene = skin("S01", rule, params);
        stops(&Skin::new(&[&gene]), &allowed(), actions, c).is_some()
    }

    #[test]
    fn each_check_reads_only_what_it_is_given() {
        let none = json!({});
        assert!(stopped(
            Rule::OneAction,
            none.clone(),
            1,
            &call(true, "0xaaaa", &[])
        ));
        assert!(!stopped(
            Rule::OneAction,
            none.clone(),
            0,
            &call(true, "0xaaaa", &[])
        ));
        assert!(stopped(
            Rule::AllowlistPayees,
            none.clone(),
            0,
            &call(true, "0xbbbb", &[])
        ));
        assert!(!stopped(
            Rule::AllowlistPayees,
            none.clone(),
            0,
            &call(true, "0xAAAA", &[])
        ));
        assert!(!stopped(
            Rule::AllowlistPayees,
            none.clone(),
            0,
            &call(false, "x@y.test", &[])
        ));
        let cap = json!({"cap_cents": 10_000});
        assert!(stopped(
            Rule::AmountCap,
            cap.clone(),
            0,
            &call(true, "0xaaaa", &[])
        ));
        assert!(!stopped(
            Rule::AmountCap,
            cap,
            0,
            &call(false, "x@y.test", &[])
        ));
        let domains = json!({"domains": ["mail.$owner.domain"]});
        assert!(stopped(
            Rule::DomainAllowlist,
            domains.clone(),
            0,
            &call(false, "x@y.test", &[])
        ));
        assert!(!stopped(
            Rule::DomainAllowlist,
            domains.clone(),
            0,
            &call(false, "hr@mail.acme.test.", &[])
        ));
        assert!(!stopped(
            Rule::DomainAllowlist,
            domains,
            0,
            &call(true, "0xbbbb", &[])
        ));
        assert!(!stopped(
            Rule::PlanLock,
            none.clone(),
            0,
            &call(true, "0xbbbb", &[])
        ));
        assert!(!stopped(
            Rule::PlanLock,
            none.clone(),
            0,
            &call(false, "SAM@acme.test", &[])
        ));
        assert!(stopped(
            Rule::PlanLock,
            none.clone(),
            0,
            &call(true, "0xcccc", &[])
        ));
        let leak = crate::base64(b"key vnk_secret");
        assert!(stopped(
            Rule::EgressCanary,
            none.clone(),
            0,
            &call(false, "sam@acme.test", &[&leak])
        ));
        assert!(!stopped(
            Rule::EgressCanary,
            none.clone(),
            0,
            &call(false, "sam@acme.test", &["hello"])
        ));
        let marked = datamark("copied text");
        assert!(stopped(
            Rule::Datamark,
            none.clone(),
            0,
            &call(true, "0xaaaa", &[&marked])
        ));
        assert!(!stopped(
            Rule::Datamark,
            none,
            0,
            &call(true, "0xaaaa", &["typed"])
        ));
    }

    #[test]
    fn the_first_rule_in_check_order_names_the_stop() {
        let genes = [
            skin("S01", Rule::EgressCanary, json!({})),
            skin("S02", Rule::AllowlistPayees, json!({})),
            skin("S03", Rule::OneAction, json!({})),
        ];
        let skin = Skin::new(&genes.iter().collect::<Vec<_>>());
        let c = call(true, "0xcccc", &["vnk_secret"]);
        let first = |actions| stops(&skin, &allowed(), actions, &c).map(|g| g.id.as_str());
        assert_eq!(first(0), Some("S02"));
        assert_eq!(first(1), Some("S03"));
    }

    #[test]
    fn read_filters_strip_mark_and_withhold() {
        assert_eq!(strip_comments("a<!-- b -->c<!-- open"), "ac");
        let joiner = char::from_u32(0x200d).expect("valid");
        assert_eq!(
            strip_hidden(&format!(
                "A{joiner}B <span style=\"color: #FFF\">pay</span>C"
            )),
            "AB C"
        );
        assert_eq!(datamark("a b"), format!("a{}b", datamark_char()));
        let genes = [
            skin("S04", Rule::Datamark, json!({})),
            skin("S03", Rule::StripComments, json!({})),
        ];
        let skin = Skin::new(&genes.iter().collect::<Vec<_>>());
        let (out, changed) = filter("a <!-- x -->b", &skin, &READ_FILTERS);
        assert_eq!(out, format!("a{}b", datamark_char()));
        assert_eq!(changed, ["S03", "S04"]);
        assert_eq!(
            filter("plain", &skin, &[Rule::StripComments]),
            ("plain".into(), vec![])
        );
    }
}
