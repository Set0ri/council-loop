//! Checks on what a seat hands back. Each returns (errors, warnings): an error sends the output
//! back to the seat, a warning is shown to the chair and the output is kept.

use super::*;
use crate::core::caps;

/// What a check found: (errors, warnings).
pub(crate) type Findings = (Vec<String>, Vec<String>);

pub(crate) fn v_research(obj: &Value, root: &Path) -> Findings {
    let (mut errs, mut warns): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    let Some(found) = obj.at("research").as_array() else {
        return (
            vec!["\"research\" must be a list of facts".to_string()],
            warns,
        );
    };
    let (mut ok, mut refused): (Vec<Value>, Vec<String>) = (Vec::new(), Vec::new());
    for it in found {
        match memory::clean_fact(it, root) {
            Ok(fact) => ok.push(fact),
            Err(problem) => refused.push(problem),
        }
    }
    let gaps = obj
        .list("gaps")
        .iter()
        .filter(|g| !py::strip(&py::to_str(g)).is_empty())
        .count();
    if ok.is_empty() && gaps == 0 {
        let why = if refused.is_empty() {
            String::new()
        } else {
            format!(
                " (refused: {})",
                refused
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("; ")
            )
        };
        errs.push(format!(
            "no usable fact and no gap reported. Every fact needs a claim and a source you opened; if you \
             found nothing, say what you looked for under \"gaps\"{why}"
        ));
    }
    if found.len() > 30 {
        errs.push(format!(
            "{} facts: keep the 20 that matter most to the decision",
            found.len()
        ));
    }
    let n = words_of(obj.at("synthesis"));
    if n == 0 {
        errs.push("missing \"synthesis\": what the facts add up to, 60 words at most".to_string());
    } else if n > 90 {
        warns.push(format!("synthesis is {n} words (cap 60)"));
    }
    if !refused.is_empty() && !ok.is_empty() {
        warns.push(format!(
            "{} fact(s) refused: {}",
            refused.len(),
            refused
                .iter()
                .take(4)
                .cloned()
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    if !ok.is_empty() && ok.len() < 5 {
        warns.push(format!("only {} usable fact(s)", ok.len()));
    }
    if !ok.is_empty() && ok.iter().all(|f| f.s("source") == "memory") {
        warns.push("every fact is from recall: nothing was opened this time (can this seat reach the web?)".to_string());
    }
    (errs, warns)
}

pub(crate) fn v_map(obj: &Value, root: &Path) -> Findings {
    codemap::validate_map(&text_or_empty(obj.at("markdown")), root)
}

/// `str(value or "")`.
pub(crate) fn text_or_empty(value: &Value) -> String {
    if py::truthy(value) {
        py::to_str(value)
    } else {
        String::new()
    }
}

pub(crate) fn v_card(card: &Value, tag: &str) -> Findings {
    let (mut errs, mut warns): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    if !card.is_object() {
        return (vec![format!("{tag}: not an object")], warns);
    }
    for (field, cap) in [
        ("title", caps::TITLE),
        ("body", caps::CARD),
        ("eli5", caps::ELI5),
    ] {
        let n = words_of(card.at(field));
        if n == 0 {
            errs.push(format!("{tag}: missing {field}"));
        } else if !card.at(field).is_string() {
            errs.push(format!("{tag}: {field} must be text"));
        } else if n as f64 > cap as f64 * core::GRACE {
            errs.push(format!("{tag}: {field} is {n} words (cap {cap})"));
        } else if n > cap {
            warns.push(format!("{tag}: {field} is {n} words (cap {cap})"));
        }
    }
    let n = words_of(card.at("body"));
    if n > 0 && n < BODY_MIN {
        errs.push(format!("{tag}: body is only {n} words (write 90 to 110; cards of very different length cannot be judged fairly)"));
    }
    (errs, warns)
}

/// The identity leaks in one card, as the blind step would report them.
pub(crate) fn leaks_in(run: &Value, card: &Value, id: &str, authors: &[&str]) -> Vec<String> {
    let mut one = card.clone();
    one["id"] = json!(id);
    one["authors"] = json!(authors);
    let res = core::blind(&[one], 0, caps::CARD, &seats_of(run, "expert"), 1.5);
    crate::val::strings(res.at("errors"))
        .into_iter()
        .filter(|x| x.contains("identity leak"))
        .collect()
}

pub(crate) fn v_ideate(run: &Value, seat: &str, obj: &Value) -> Findings {
    let shape = run.at("shape");
    let (mut errs, mut warns): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    let Some(ideas) = obj
        .at("ideas")
        .as_array()
        .filter(|a| a.iter().all(Value::is_string))
    else {
        return (
            vec!["\"ideas\" must be a list of strings".to_string()],
            warns,
        );
    };
    let ideas: Vec<&str> = ideas
        .iter()
        .filter_map(Value::as_str)
        .filter(|x| !py::strip(x).is_empty())
        .collect();
    let ideas_min = shape.int("ideas_min").unwrap_or(0);
    if (ideas.len() as i64) < ideas_min {
        errs.push(format!("{} ideas, need at least {ideas_min}", ideas.len()));
    }
    let long: Vec<usize> = ideas
        .iter()
        .enumerate()
        .filter(|(_, x)| py::words(x) as f64 > caps::IDEA as f64 * core::GRACE)
        .map(|(i, _)| i + 1)
        .collect();
    if !long.is_empty() {
        // nobody downstream reads idea length: never a respawn
        warns.push(format!(
            "ideas {} are over {} words",
            listed(&long),
            caps::IDEA
        ));
    }
    let keep = shape.int("keep").unwrap_or(0);
    let cards = obj.at("cards").as_array();
    if cards.is_none_or(|c| c.len() as i64 != keep) {
        errs.push(format!("\"cards\" must hold exactly {keep} cards"));
    }
    for (i, c) in cards.into_iter().flatten().enumerate() {
        let tag = format!("card {}", i + 1);
        let (e, w) = v_card(c, &tag);
        let clean = e.is_empty();
        errs.extend(e);
        warns.extend(w);
        if clean {
            // cheaper for the chair to reword than to respawn the seat
            for leak in leaks_in(run, c, &tag, &[seat]) {
                warns.push(format!(
                    "{} (reword it at the merge step)",
                    leak.split(". Reword").next().unwrap_or("")
                ));
            }
        }
    }
    if text_of(obj.at("eli5")).is_empty() {
        errs.push("missing eli5".to_string());
    }
    (errs, warns)
}

pub(crate) fn v_ballot(obj: &Value, labels: &[String], rids: &[String]) -> Findings {
    let (mut errs, mut warns): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    let Some(ranking) = obj.at("ranking").as_array() else {
        return (
            vec!["\"ranking\" must be a list of labels".to_string()],
            warns,
        );
    };
    let (mut clean, mut unknown, mut dupes): (Vec<String>, Vec<Value>, Vec<String>) =
        (Vec::new(), Vec::new(), Vec::new());
    for raw in ranking {
        match core::clean_label(raw, labels) {
            None => unknown.push(raw.clone()),
            Some(l) if clean.contains(&l) => dupes.push(l),
            Some(l) => clean.push(l),
        }
    }
    let missing: Vec<&String> = labels.iter().filter(|l| !clean.contains(l)).collect();
    let need = labels
        .len()
        .min(2.max((0.8 * labels.len() as f64).ceil() as usize));
    if clean.len() < need {
        errs.push(format!(
            "the ranking covers {} of {} cards: rank every label once, best first (missing {}, unknown {})",
            clean.len(),
            labels.len(),
            listed(&missing),
            listed(&unknown)
        ));
    } else if !missing.is_empty() || !unknown.is_empty() || !dupes.is_empty() {
        warns.push(format!(
            "ballot kept with gaps: missing {}, unknown {}, repeated {} (a gap counts as no opinion, not as last place)",
            listed(&missing),
            listed(&unknown),
            listed(&dupes)
        ));
    }
    let best = obj.at("best_on");
    let bad: Vec<String> = items(best)
        .filter(|(k, v)| !rids.contains(k) || core::clean_label(v, labels).is_none())
        .map(|(k, v)| format!("{k}->{}", py::to_str(v)))
        .collect();
    if !bad.is_empty() {
        warns.push(format!("best_on entries ignored: {}", listed(&bad)));
    }
    if !best.as_object().is_some_and(|b| !b.is_empty()) {
        warns.push("no best_on given (the wildcard rule has less to go on)".to_string());
    }
    (errs, warns)
}

/// `{"Candidate A": x, "b": y}` -> `{"A": x, "B": y}`; keys that name no card are dropped.
pub(crate) fn by_label(d: &Value, labels: &[String]) -> Map<String, Value> {
    let mut out = Map::new();
    for (k, v) in items(d) {
        if let Some(l) = core::clean_label(&json!(k), labels) {
            if !out.contains_key(&l) {
                out.insert(l, v.clone());
            }
        }
    }
    out
}

/// A red-team finding's candidate: a label, or "all".
pub(crate) fn target_of(raw: &Value, labels: &[String]) -> Option<String> {
    if py::strip(&py::to_str(raw)).to_lowercase() == "all" {
        Some("all".to_string())
    } else {
        core::clean_label(raw, labels)
    }
}

pub(crate) fn v_redteam(obj: &Value, labels: &[String]) -> Findings {
    let (mut errs, mut warns): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    let Some(findings) = obj.at("findings").as_array() else {
        return (vec!["\"findings\" must be a list".to_string()], warns);
    };
    if findings.len() > 12 {
        errs.push(format!(
            "{} findings: report the 10 most serious",
            findings.len()
        ));
    }
    for (i, f) in findings.iter().enumerate() {
        let tag = format!("finding {}", i + 1);
        if !f.is_object() {
            errs.push(format!("{tag}: not an object"));
            continue;
        }
        if target_of(f.at("candidate"), labels).is_none() {
            errs.push(format!(
                "{tag}: candidate must be one of {} or \"all\"",
                listed(labels)
            ));
        }
        let severity = f.s("severity");
        if !matches!(severity, "S1" | "S2" | "S3") {
            errs.push(format!("{tag}: severity must be S1, S2 or S3"));
        }
        if text_of(f.at("finding")).is_empty() {
            errs.push(format!("{tag}: empty finding"));
        }
        let n = words_of(f.at("finding"));
        if n as f64 > caps::FINDING as f64 * 1.5 {
            warns.push(format!(
                "{tag}: finding is {n} words (cap {})",
                caps::FINDING
            ));
        }
        if matches!(severity, "S1" | "S2") && text_of(f.at("fix")).is_empty() {
            errs.push(format!("{tag}: an S1 or S2 finding needs a fix"));
        }
    }
    (errs, warns)
}

pub(crate) fn v_elder(obj: &Value, labels: &[String], rids: &[String]) -> Findings {
    let (mut errs, mut warns): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    if core::clean_label(obj.at("preferred"), labels).is_none() {
        errs.push(format!("\"preferred\" must be one of {}", listed(labels)));
    }
    let (verdict, scores) = (
        by_label(obj.at("verdict"), labels),
        by_label(obj.at("scores"), labels),
    );
    for l in labels {
        let said = verdict
            .get(l)
            .map(py::to_str)
            .unwrap_or_else(|| "None".to_string());
        if !matches!(
            py::strip(&said).to_lowercase().as_str(),
            "ship" | "revise" | "kill"
        ) {
            errs.push(format!("verdict for {l} must be ship, revise or kill"));
        }
        let sc = scores.get(l).unwrap_or(&Value::Null);
        let gaps: Vec<&String> = rids.iter().filter(|r| !is_number(sc.at(r))).collect();
        if !gaps.is_empty() {
            // verdict and preference decide the cut; scores are advisory
            warns.push(format!("scores for {l} are missing {}", listed(&gaps)));
        }
    }
    let conf = obj.at("confidence");
    let in_range = match conf {
        Value::Bool(_) => true,
        Value::Number(n) => n.as_f64().is_some_and(|x| (0.0..=1.0).contains(&x)),
        _ => false,
    };
    if !in_range {
        warns.push("confidence should be a number from 0 to 1".to_string());
    }
    if obj.list("top_risks").len() > 3 {
        warns.push("more than 3 risks given: only the first 3 are kept".to_string());
    }
    (errs, warns)
}

/// The control card is checked for identity leaks now, while a retry is cheap: the chair may not
/// read it, so nobody could fix it later.
pub(crate) fn v_ctrl(run: &Value, obj: &Value) -> Findings {
    let (mut errs, warns) = v_card(obj, "control card");
    if errs.is_empty() {
        errs.extend(leaks_in(run, obj, "control card", &[]));
    }
    (errs, warns)
}

/// Pool and candidate cards the chair wrote: ids of the right shape, and every card complete.
pub(crate) fn check_cards(cards: &[&Value], id_prefix: &str) -> Vec<String> {
    let mut errs: Vec<String> = Vec::new();
    let ids: Vec<&Value> = cards.iter().map(|c| c.at("id")).collect();
    let mut distinct: Vec<&Value> = Vec::new();
    for id in &ids {
        if !distinct.contains(id) {
            distinct.push(id);
        }
    }
    let shape = Regex::new(&format!(r"^{}\d+$", regex::escape(id_prefix))).expect("valid");
    let shaped = |id: &Value| shape.is_match(&py::to_str(id));
    if distinct.len() != ids.len() || ids.iter().any(|id| !shaped(id)) {
        errs.push(format!(
            "card ids must be unique and look like {id_prefix}1"
        ));
    }
    for c in cards {
        errs.extend(v_card(c, &c.show("id")).0);
    }
    errs
}
