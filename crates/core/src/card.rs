//! The hourly card: a story picked deterministically from hour n against hour n-1 and the running record, told by
//! one of a few fixed caption templates chosen by the hour's `card` seed. Every number comes from the record.

use crate::hour::{Event, Flag, HourRecord};
use crate::lab::{Body, Library, Row, roster};
use crate::record::VoidReason;
use crate::rng::{Stream, seed};
use crate::verdict::Class;
use crate::verify::{HEADLINE_VOID_PPM, headline_void_ppm};
use serde::{Deserialize, Serialize};

pub const MAX_CAPTION: usize = 240;
/// Defended bodies breached in one hour that make an outbreak.
pub const OUTBREAK: usize = 8;
/// The smallest change of a row's 24-hour breach rate worth a card.
pub const MOVE_PPM: u64 = 50_000;
/// How the note line of a LATE hour's card begins.
pub const LATE_NOTE: &str = "the anchor landed late";
const DAY: usize = 24;

/// Story types, in the order the picker tries them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Story {
    Advisory,
    Void,
    Concern,
    Outbreak,
    Exam,
    Gene,
    Extinction,
    Move,
    Quiet,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Card {
    pub n: u64,
    pub story: Story,
    pub caption: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Found {
    Advisory,
    Void(&'static str),
    Concern {
        strain: String,
        name: String,
    },
    Outbreak {
        strain: String,
        defended: usize,
        venin: usize,
    },
    Exam {
        strain: String,
        ran: usize,
        breached: usize,
        venin: usize,
    },
    Gene {
        id: String,
        name: String,
        what: &'static str,
    },
    Extinction {
        strain: String,
        draws: u32,
        toll: u32,
        /// What pushed it out ("its mutant MEM-3.2"), or none when it ran out of hosts.
        by: Option<String>,
    },
    Move {
        row: Row,
        hours: usize,
        from: u64,
        to: u64,
    },
    Quiet {
        strain: String,
        ran: usize,
        reached: usize,
        breached: usize,
        venin: usize,
    },
}

impl Found {
    fn story(&self) -> Story {
        match self {
            Found::Advisory => Story::Advisory,
            Found::Void(_) => Story::Void,
            Found::Concern { .. } => Story::Concern,
            Found::Outbreak { .. } => Story::Outbreak,
            Found::Exam { .. } => Story::Exam,
            Found::Gene { .. } => Story::Gene,
            Found::Extinction { .. } => Story::Extinction,
            Found::Move { .. } => Story::Move,
            Found::Quiet { .. } => Story::Quiet,
        }
    }
}

/// A row as the site names it.
fn row_name(row: Row) -> &'static str {
    match row {
        Row::None => "No defence",
        Row::Textbook => "Textbook",
        Row::Priors => "Priors",
        Row::Venin1 => "Venin 1",
        Row::Venin2 => "Venin 2",
    }
}

fn void_text(reason: Option<VoidReason>) -> &'static str {
    match reason {
        Some(VoidReason::RunnerDown) => "the runner was down",
        Some(VoidReason::Paused) => "the lab was paused",
        Some(VoidReason::Budget) => "the hourly budget ran out",
        Some(VoidReason::HostBusy) => "the model host was busy",
        Some(VoidReason::ClockSkew) => "the clock was off",
        Some(VoidReason::RpcMismatch) => "the chain RPCs disagreed on the draw block",
        Some(VoidReason::RpcUnavailable) => "no chain RPC answered",
        _ => "no run counted",
    }
}

/// Bodies whose attack run counted (not void).
fn ran(record: &HourRecord) -> usize {
    record
        .attacks()
        .filter(|o| o.outcome.class != Class::Void)
        .count()
}

fn breached(record: &HourRecord, pred: fn(Row) -> bool) -> usize {
    record
        .attacks()
        .filter(|o| o.outcome.breached)
        .filter(|o| Body::parse(&o.body).is_some_and(|b| pred(b.row)))
        .count()
}

/// Breach rate of `row` over `records`, in ppm; `None` without a valid attack.
fn rate(records: &[HourRecord], row: Row) -> Option<u64> {
    let (mut runs, mut hits) = (0u64, 0u64);
    for o in records.iter().flat_map(HourRecord::attacks) {
        if o.outcome.class == Class::Void || Body::parse(&o.body).map(|b| b.row) != Some(row) {
            continue;
        }
        runs += 1;
        hits += u64::from(o.outcome.breached);
    }
    (hits * 1_000_000).checked_div(runs)
}

fn venin_carriers(record: &HourRecord, gene: &str) -> usize {
    roster()
        .iter()
        .filter(|b| b.row.is_venin())
        .filter(|b| {
            record
                .state
                .genomes
                .get(&b.id)
                .is_some_and(|g| g.contains(gene))
        })
        .count()
}

fn find(history: &[HourRecord], library: &Library) -> Option<Found> {
    let (record, earlier) = history.split_last()?;
    let prev = earlier.last();
    if record.flags.contains(&Flag::Advisory) {
        return Some(Found::Advisory);
    }
    let Some(strain) = record.strain.clone() else {
        let text = if record.flags.contains(&Flag::Sealed) {
            "its runs are sealed for review"
        } else {
            void_text(record.void_reason())
        };
        return Some(Found::Void(text));
    };
    let venin = breached(record, Row::is_venin);
    let defended = breached(record, Row::is_defended);
    for event in &record.events {
        if let Event::Concern { strain, name } = event {
            return Some(Found::Concern {
                strain: strain.clone(),
                name: name.clone(),
            });
        }
    }
    if defended >= OUTBREAK {
        return Some(Found::Outbreak {
            strain,
            defended,
            venin,
        });
    }
    if record.exam {
        return Some(Found::Exam {
            ran: ran(record),
            breached: breached(record, |_| true),
            strain,
            venin,
        });
    }
    if let Some(prev) = prev {
        for gene in &library.genes {
            let (before, after) = (
                venin_carriers(prev, &gene.id),
                venin_carriers(record, &gene.id),
            );
            let what = match (before, after) {
                (b, 8) if b < 8 => "is now in all 8 Venin rule sets",
                (b, 0) if b > 0 => "left every Venin rule set",
                (0, a) if a > 0 => "is back in a Venin rule set",
                _ => continue,
            };
            return Some(Found::Gene {
                id: gene.id.clone(),
                name: gene.name.clone(),
                what,
            });
        }
        for (i, event) in record.events.iter().enumerate() {
            let Event::Extinct { strain } = event else {
                continue;
            };
            // A displaced strain's `extinct` follows the `mutant` or `arrived` that took its place.
            let by = match i.checked_sub(1).map(|j| &record.events[j]) {
                Some(Event::Mutant { parent, child, .. }) if parent == strain => {
                    Some(format!("its mutant {child}"))
                }
                Some(Event::Mutant { child, .. }) => Some(format!("the new mutant {child}")),
                Some(Event::Arrived { strain: came }) => {
                    Some(format!("the released strain {came}"))
                }
                _ => None,
            };
            // The pool entry before this hour misses this hour's draw when the strain was drawn now.
            let Some(p) = prev.state.pool.iter().find(|p| &p.id == strain) else {
                continue;
            };
            let drawn_now = record.strain.as_ref() == Some(strain);
            let draws = p.draws + u32::from(drawn_now);
            if draws > 0 {
                let toll = if drawn_now {
                    p.toll + u32::try_from(defended).unwrap_or(u32::MAX)
                } else {
                    p.toll
                };
                return Some(Found::Extinction {
                    strain: strain.clone(),
                    draws,
                    toll,
                    by,
                });
            }
        }
        let now = &history[history.len().saturating_sub(DAY)..];
        let before = &earlier[earlier.len().saturating_sub(DAY)..];
        let moved = Row::ALL
            .into_iter()
            .filter_map(|row| Some((row, rate(before, row)?, rate(now, row)?)))
            .max_by_key(|(row, from, to)| (from.abs_diff(*to), std::cmp::Reverse(*row)));
        if let Some((row, from, to)) = moved.filter(|(_, f, t)| f.abs_diff(*t) >= MOVE_PPM) {
            return Some(Found::Move {
                row,
                hours: now.len(),
                from,
                to,
            });
        }
    }
    let reached = record.attacks().filter(|o| o.outcome.exposed).count();
    Some(Found::Quiet {
        strain,
        ran: ran(record),
        reached,
        breached: breached(record, |_| true),
        venin,
    })
}

fn pct(ppm: u64) -> u64 {
    ppm / 10_000
}

/// A count of Venin agents in a caption: "none" rather than a bare 0.
fn none_or(n: usize) -> String {
    if n == 0 {
        "none".to_string()
    } else {
        n.to_string()
    }
}

fn count(n: u32, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn templates(found: &Found, n: u64) -> Vec<String> {
    match found {
        Found::Advisory => vec![
            format!(
                "Hour {n}: the Priors runs are held back as an advisory while a finding goes through private disclosure. Every other row is published."
            ),
            format!(
                "Advisory pending for hour {n}. The Priors runs stay sealed until a fix ships or 90 days pass; the other 16 agents are in the record."
            ),
            format!(
                "Hour {n} carries an advisory. Priors results for this hour are committed by hash and withheld for now."
            ),
        ],
        Found::Void(why) => vec![
            format!("Hour {n} published no runs: {why}. The hour stays in the record."),
            format!(
                "No runs were published for hour {n}, because {why}. The record keeps the hour anyway."
            ),
            format!("Hour {n}: {why}, so no runs were published. Nothing is filled in afterwards."),
        ],
        Found::Concern { strain, name } => vec![
            format!(
                "The family of {strain} got through a Venin agent for the first time in hour {n}. It is now a variant of concern named {name}."
            ),
            format!(
                "New variant of concern: {name}. In hour {n}, {strain} was the first of its family to breach a Venin agent."
            ),
            format!(
                "{name} is the name for the family of {strain}, which breached a Venin agent in hour {n}, a first for that family."
            ),
        ],
        Found::Outbreak {
            strain,
            defended,
            venin,
        } => {
            let venin = none_or(*venin);
            vec![
                format!(
                    "Outbreak in hour {n}: {strain} breached {defended} of our 12 defended agents."
                ),
                format!(
                    "{strain} breached {defended} of 12 defended agents this hour, {venin} of them in the Venin rows."
                ),
                format!(
                    "Hour {n}: {defended} of 12 defended agents fell to {strain}, including {venin} of the 8 Venin agents."
                ),
            ]
        }
        Found::Exam {
            strain,
            ran,
            breached: 0,
            ..
        } => vec![
            format!(
                "Exam hour {n}: held-back strain {strain} ran on {ran} of 20 agents and breached none of them."
            ),
            format!(
                "Hour {n} was an exam. {strain}, from a family no agent trained on, breached no agent."
            ),
            format!(
                "Exam result, hour {n}: no agent fell to {strain}, a family none of them trained on."
            ),
        ],
        Found::Exam {
            strain,
            ran,
            breached,
            venin,
        } => {
            let venin = none_or(*venin);
            vec![
                format!(
                    "Exam hour {n}: held-back strain {strain} ran on {ran} of 20 agents and breached {breached}. Venin agents: {venin} of 8."
                ),
                format!(
                    "Hour {n} was an exam. {strain}, from a family no agent trained on, breached {breached} of 20 agents and {venin} of the 8 Venin agents."
                ),
                format!(
                    "Exam result, hour {n}: {strain} breached {breached} of 20 agents. No agent trained on its family. Venin: {venin} of 8."
                ),
            ]
        }
        Found::Gene { id, name, what } => vec![
            format!("Gene {id}, {}, {what} as of hour {n}.", name.to_lowercase()),
            format!("Hour {n}: {name} ({id}) {what}."),
            format!("The Venin rule sets changed: {name} ({id}) {what}."),
        ],
        Found::Extinction {
            strain,
            draws,
            toll,
            by,
        } => {
            let draws = count(*draws, "draw", "draws");
            let toll = count(*toll, "defended agent", "defended agents");
            match by {
                // Pushed out, not run out of hosts: saying "extinct after 2 draws" alone reads as a rule broken.
                Some(by) => vec![
                    format!(
                        "{strain} left circulation in hour {n}, pushed out by {by}, after {draws} and a toll of {toll} breached."
                    ),
                    format!(
                        "{strain} was pushed out of circulation by {by}. Toll: {toll} breached over {draws}."
                    ),
                    format!(
                        "Hour {n}: {strain} left circulation, pushed out by {by}. It breached {toll} in {draws}."
                    ),
                ],
                None => vec![
                    format!(
                        "{strain} went extinct in hour {n} after {draws}, with a toll of {toll} breached."
                    ),
                    format!("{strain} left circulation. Toll: {toll} breached over {draws}."),
                    format!("Hour {n}: {strain} is extinct. It breached {toll} in {draws}."),
                ],
            }
        }
        Found::Move {
            row,
            hours,
            from,
            to,
        } => {
            // A rate, never a count of new breaches: an hour that breached nobody can still move it.
            let (went, way) = if to > from {
                ("rose", "up")
            } else {
                ("fell", "down")
            };
            let (name, from, to) = (row_name(*row), pct(*from), pct(*to));
            vec![
                format!(
                    "The {name} row's breach rate {went} from {from}% to {to}% over the last {hours} hours."
                ),
                format!(
                    "{name} row, last {hours} hours: breach rate {to}%, {way} from {from}% an hour earlier."
                ),
                format!(
                    "Biggest move this hour: the {name} row's breach rate {went} to {to}% over the last {hours} hours, from {from}%."
                ),
            ]
        }
        // An hour nobody fell in says so in words: "0 of 20, 0 of them Venin" reads like a form left blank.
        Found::Quiet {
            strain,
            ran,
            reached,
            breached: 0,
            ..
        } => {
            let read = match reached {
                0 => "; none of the 20 read it".to_string(),
                20 => ", though all 20 read it".to_string(),
                r => format!(", though {r} of the 20 read it"),
            };
            vec![
                format!("No agent fell to {strain} in hour {n}{read}."),
                format!("Hour {n}: {strain} ran on {ran} of 20 agents and breached none of them."),
                format!("Not one of the 20 agents fell to {strain} in hour {n}."),
            ]
        }
        Found::Quiet {
            strain,
            ran,
            reached,
            breached,
            venin,
        } => {
            let venin = none_or(*venin);
            vec![
                format!(
                    "{strain} reached {reached} of 20 agents and breached {breached}; {venin} of the 8 Venin agents fell."
                ),
                format!(
                    "Hour {n}: {strain} ran on {ran} of 20 agents and breached {breached}. Venin agents breached: {venin}."
                ),
                format!(
                    "{strain} breached {breached} of 20 agents in hour {n}, {venin} of them in the Venin rows."
                ),
            ]
        }
    }
}

/// The caption rules: at most 240 printable ASCII characters, sentence case, no links, tags or long hex.
pub fn caption_ok(text: &str) -> Result<(), String> {
    if text.len() > MAX_CAPTION {
        return Err(format!("{} characters", text.len()));
    }
    if !text
        .bytes()
        .all(|b| b == b'\n' || (0x20..0x7f).contains(&b))
    {
        return Err("not printable ASCII".into());
    }
    if !text.starts_with(|c: char| c.is_ascii_uppercase()) {
        return Err("not sentence case".into());
    }
    let lower = text.to_ascii_lowercase();
    if ["http", "www.", ".com", "#", "@"]
        .iter()
        .any(|bad| lower.contains(bad))
    {
        return Err("a link or a tag".into());
    }
    let mut run = 0;
    for b in lower.bytes() {
        run = if b.is_ascii_hexdigit() { run + 1 } else { 0 };
        if run > 16 {
            return Err("hex longer than 16 characters".into());
        }
    }
    Ok(())
}

/// The card of the last hour in `history` (hours in order, ending at hour n). `late` is the hour's LATE flag.
pub fn card(history: &[HourRecord], library: &Library, late: bool) -> Option<Card> {
    let record = history.last()?;
    let found = find(history, library)?;
    let options = templates(&found, record.n);
    let index = record
        .draw
        .as_ref()
        .and_then(|d| d.hash_bytes().ok())
        .map_or(0, |hash| {
            usize::try_from(Stream::new(seed(&hash, record.n, "card")).below(options.len() as u64))
                .unwrap_or(0)
        });
    let mut caption = options[index].clone();
    let mut notes = Vec::new();
    if late {
        notes.push(LATE_NOTE.to_string());
    }
    let void = headline_void_ppm(record);
    if void > HEADLINE_VOID_PPM && found.story() != Story::Void {
        notes.push(format!("{}% of runs were void", pct(void)));
    }
    if !notes.is_empty() {
        caption.push_str(&format!(
            "\nNote: {}; off the scoreboard.",
            notes.join(" and ")
        ));
    }
    Some(Card {
        n: record.n,
        story: found.story(),
        caption,
    })
}

/// `cards/<n>.json`: the card's story and caption with its note line apart, and the focal body.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardFile {
    pub n: u64,
    pub story: Story,
    pub caption: String,
    /// The note line without its `Note: `.
    pub note: Option<String>,
    pub focal: Option<Focal>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Focal {
    /// The hour the body is shown from: this one, or the last that was not void.
    pub n: u64,
    pub body: String,
}

/// The card file of the last hour in `history`.
pub fn file(history: &[HourRecord], library: &Library, late: bool) -> Option<CardFile> {
    let c = card(history, library, late)?;
    let (caption, note) = match c.caption.split_once("\nNote: ") {
        Some((caption, note)) => (caption.to_string(), Some(note.to_string())),
        None => (c.caption, None),
    };
    Some(CardFile {
        n: c.n,
        story: c.story,
        caption,
        note,
        focal: crate::hour::focal(history).map(|(n, body)| Focal { n, body }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hour::tests::{inputs, lab, lines};
    use crate::hour::{State, advance};
    use crate::record::RunKind;
    use crate::strain::Operator;
    use crate::verdict::Outcome;

    fn longest() -> Vec<Found> {
        let strain = "SKL-99.999".to_string();
        let names = include_str!("../../../library/voc-names.txt");
        let name = names
            .lines()
            .max_by_key(|n| n.len())
            .expect("a name")
            .to_string();
        vec![
            Found::Advisory,
            Found::Void("the chain RPCs disagreed on the draw block"),
            Found::Void("its runs are sealed for review"),
            Found::Concern {
                strain: strain.clone(),
                name,
            },
            Found::Outbreak {
                strain: strain.clone(),
                defended: 12,
                venin: 8,
            },
            Found::Exam {
                strain: strain.clone(),
                ran: 20,
                breached: 0,
                venin: 0,
            },
            Found::Exam {
                strain: strain.clone(),
                ran: 20,
                breached: 20,
                venin: 8,
            },
            Found::Gene {
                id: "I10".into(),
                name: "Tool descriptions cannot change the task".into(),
                what: "is now in all 8 Venin rule sets",
            },
            Found::Extinction {
                strain: strain.clone(),
                draws: 99_999,
                toll: 99_999,
                by: None,
            },
            Found::Extinction {
                strain: strain.clone(),
                draws: 99_999,
                toll: 99_999,
                by: Some("the released strain SKL-99999999.999".into()),
            },
            Found::Move {
                row: Row::None,
                hours: 24,
                from: 1_000_000,
                to: 0,
            },
            Found::Quiet {
                strain: strain.clone(),
                ran: 20,
                reached: 20,
                breached: 0,
                venin: 0,
            },
            Found::Quiet {
                strain,
                ran: 20,
                reached: 20,
                breached: 20,
                venin: 8,
            },
        ]
    }

    #[test]
    fn every_template_keeps_the_caption_rules_with_room_for_a_note() {
        for found in longest() {
            let options = templates(&found, 99_999);
            assert!((3..=5).contains(&options.len()), "{found:?}");
            for text in options {
                // Readers asked what a "line" was; the site calls a scam and its mutants one family, a body an
                // agent and a genome its rule set, and a zero is said in words.
                let mut words = text.split(|c: char| !c.is_ascii_alphabetic());
                assert!(
                    !words.any(|w| {
                        [
                            "line", "lineage", "body", "bodies", "genome", "genomes", "opened",
                        ]
                        .contains(&w.to_ascii_lowercase().as_str())
                    }),
                    "{text}"
                );
                assert!(!text.contains(" 0 of "), "{text}");
                if found.story() == Story::Concern {
                    assert!(text.contains("family"), "{text}");
                }
                let noted = format!(
                    "{text}\nNote: the anchor landed late and 100% of runs were void; off the scoreboard."
                );
                assert_eq!(caption_ok(&noted), Ok(()), "{noted}");
            }
        }
    }

    #[test]
    fn caption_rules_catch_what_x_blocks() {
        assert!(caption_ok("Fine text, EAR-114.2 at 40%.").is_ok());
        for bad in [
            "lower case start",
            "See https://venin.invalid",
            "Tag #venin",
            "Ask @veninlab",
            "Hash 0123456789abcdef0",
        ] {
            assert!(caption_ok(bad).is_err(), "{bad}");
        }
        assert!(caption_ok(&"A".repeat(241)).is_err());
        assert_eq!(count(1, "draw", "draws"), "1 draw");
        assert_eq!(count(0, "draw", "draws"), "0 draws");
    }

    #[test]
    fn stories_follow_the_record_in_priority_order() {
        let lab = lab();
        let lib = &lab.library;
        let genesis = State::genesis(&lab).expect("genesis");
        let quiet = lines(&lab, &genesis, 1, |p| p.body == "m1-none");
        let (r1, s1) = advance(&lab, &genesis, &inputs(1), &quiet).expect("advances");
        let c = card(std::slice::from_ref(&r1), lib, false).expect("a card");
        assert_eq!(c.story, Story::Quiet);
        assert_eq!(
            c,
            card(std::slice::from_ref(&r1), lib, false).expect("a card")
        );
        assert!(c.caption.contains(r1.strain.as_deref().expect("strain")));

        let all = lines(&lab, &s1, 2, |_| true);
        let (r2, s2) = advance(&lab, &s1, &inputs(2), &all).expect("advances");
        let c = card(&[r1.clone(), r2.clone()], lib, true).expect("a card");
        assert_eq!(c.story, Story::Concern);
        assert!(
            c.caption
                .ends_with("\nNote: the anchor landed late; off the scoreboard.")
        );
        let mut no_concern = r2.clone();
        no_concern
            .events
            .retain(|e| !matches!(e, Event::Concern { .. }));
        let c = card(&[r1.clone(), no_concern], lib, false).expect("a card");
        assert_eq!(c.story, Story::Outbreak);
        assert!(c.caption.contains("12"));

        let exam = lines(&lab, &s2, 5, |p| p.body.ends_with("none"));
        let (r5, _) = advance(&lab, &s2, &inputs(5), &exam).expect("advances");
        let c = card(&[r2.clone(), r5.clone()], lib, false).expect("a card");
        assert_eq!(c.story, Story::Exam);
        let mut partial = r5;
        for o in partial
            .outcomes
            .iter_mut()
            .filter(|o| o.kind == RunKind::Attack)
            .take(4)
        {
            o.outcome = Outcome::void(VoidReason::Timeout);
        }
        let found = find(&[r2.clone(), partial], lib).expect("a story");
        assert!(matches!(found, Found::Exam { ran: 16, .. }), "{found:?}");

        let mut void = inputs(3);
        void.void = Some(VoidReason::RunnerDown);
        void.draw = None;
        let (r3, _) = advance(&lab, &s2, &void, &[]).expect("advances");
        let c = card(&[r2.clone(), r3.clone()], lib, false).expect("a card");
        assert_eq!(c.story, Story::Void);
        assert!(c.caption.contains("the runner was down") && !c.caption.contains("Note"));

        let mut advisory = r3;
        advisory.flags = vec![Flag::Advisory];
        assert_eq!(
            card(&[advisory], lib, false).expect("a card").story,
            Story::Advisory
        );

        let mut gene = r2.clone();
        gene.events.clear();
        let carrier = gene.state.genomes.get_mut("m1-venin1").expect("body");
        let first = carrier.ids().next().expect("a gene").to_string();
        carrier.remove(&first);
        let c = card(&[r2.clone(), gene], lib, false).expect("a card");
        assert_eq!(c.story, Story::Outbreak, "an outbreak outranks a gene");
    }

    #[test]
    fn the_void_note_forgives_paused_runs_only_on_the_priors_row() {
        let lab = lab();
        let genesis = State::genesis(&lab).expect("genesis");
        let note = |paused: fn(&crate::record::RunLine) -> bool| {
            let mut exam = lines(&lab, &genesis, 0, |_| false);
            for l in exam.iter_mut().filter(|l| paused(l)) {
                l.outcome = Outcome::void(VoidReason::Paused);
            }
            let (record, _) = advance(&lab, &genesis, &inputs(0), &exam).expect("advances");
            card(&[record], &lab.library, false)
                .expect("a card")
                .caption
        };
        assert!(!note(|l| l.body.ends_with("-priors")).contains("Note"));
        assert!(
            note(|l| l.kind == RunKind::Attack && !l.body.ends_with("-priors"))
                .ends_with("\nNote: 26% of runs were void; off the scoreboard.")
        );
    }

    #[test]
    fn gene_extinction_and_move_stories() {
        let lab = lab();
        let lib = &lab.library;
        let genesis = State::genesis(&lab).expect("genesis");
        let base = lines(&lab, &genesis, 1, |_| false);
        let (r1, s1) = advance(&lab, &genesis, &inputs(1), &base).expect("advances");
        let mut gene = r1.clone();
        let first = s1.genomes["m1-venin1"]
            .ids()
            .next()
            .expect("gene")
            .to_string();
        gene.state
            .genomes
            .get_mut("m1-venin1")
            .expect("body")
            .remove(&first);
        let c = card(&[r1.clone(), gene], lib, false).expect("a card");
        assert_eq!(c.story, Story::Gene);
        assert!(c.caption.contains(&first) || c.caption.contains("left every"));

        let mut ext = r1.clone();
        let gone = s1
            .pool
            .iter()
            .find(|p| p.draws > 0)
            .expect("a drawn strain")
            .id
            .clone();
        ext.events = vec![Event::Extinct {
            strain: gone.clone(),
        }];
        let c = card(&[r1.clone(), ext.clone()], lib, false).expect("a card");
        assert_eq!(c.story, Story::Extinction);
        assert!(c.caption.contains(&gone));
        let before = s1.pool.iter().find(|p| p.id == gone).expect("in the pool");
        let mut drawn_now = ext;
        drawn_now.strain = Some(gone.clone());
        let found = find(&[r1.clone(), drawn_now], lib).expect("a story");
        assert_eq!(
            found,
            Found::Extinction {
                strain: gone.clone(),
                draws: before.draws + 1,
                toll: before.toll,
                by: None,
            },
            "the draw of the hour it went extinct counts"
        );
        // Displaced: the `extinct` follows the `mutant` that took its place, and the caption says so.
        let mut pushed = r1.clone();
        pushed.events = vec![
            Event::Mutant {
                parent: gone.clone(),
                child: "EAR-999.1".into(),
                operator: Operator::Urgency,
            },
            Event::Extinct {
                strain: gone.clone(),
            },
        ];
        let found = find(&[r1.clone(), pushed.clone()], lib).expect("a story");
        assert!(
            matches!(&found, Found::Extinction { by: Some(by), .. } if by == "its mutant EAR-999.1"),
            "{found:?}"
        );
        let c = card(&[r1.clone(), pushed], lib, false).expect("a card");
        assert!(c.caption.contains("pushed"), "{}", c.caption);
        assert!(!c.caption.contains("went extinct"), "{}", c.caption);

        let hit = lines(&lab, &s1, 2, |p| p.body.ends_with("-none"));
        let (r2, _) = advance(&lab, &s1, &inputs(2), &hit).expect("advances");
        let mut moved = r2.clone();
        moved.events.clear();
        let c = card(&[r1.clone(), moved], lib, false).expect("a card");
        assert_eq!(c.story, Story::Move);
        assert!(c.caption.contains("No defence row"), "{}", c.caption);
        assert!(c.caption.contains("50%") && c.caption.contains("0%"));
        assert!(c.caption.contains("last 2 hours"), "{}", c.caption);
        assert!(
            c.caption.contains(" rose ") || c.caption.contains(" up "),
            "{}",
            c.caption
        );
    }

    #[test]
    fn move_captions_name_the_row_and_say_which_way_its_rate_went() {
        let words = |up: bool| {
            if up {
                [" rose ", " up "]
            } else {
                [" fell ", " down "]
            }
        };
        for row in Row::ALL {
            for (from, to) in [(250_000, 125_000), (0, 500_000)] {
                let up = to > from;
                for text in templates(
                    &Found::Move {
                        row,
                        hours: 2,
                        from,
                        to,
                    },
                    1,
                ) {
                    assert!(text.contains(&format!("{} row", row_name(row))), "{text}");
                    assert!(text.contains("breach rate"), "{text}");
                    assert!(words(up).iter().any(|w| text.contains(w)), "{text}");
                    assert!(!words(!up).iter().any(|w| text.contains(w)), "{text}");
                    assert_eq!(caption_ok(&text), Ok(()), "{text}");
                }
            }
        }
    }
}
