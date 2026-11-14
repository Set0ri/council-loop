//! Seats: which model sits where, and how to give it a fresh context from this machine.

use super::*;

/// A fresh context for a seat with no model of its own: a subagent, a command-line tool, or the chair itself.
pub(crate) fn fresh(env: &Value) -> Map<String, Value> {
    let how = env.s("fresh_contexts");
    let value = if how == "subagents" {
        json!({"via": "subagent", "alias": "inherit"})
    } else if let Some(cli) = how.strip_prefix("cli:") {
        let cli = cli.split(':').next().unwrap_or(cli);
        let provider = match cli {
            "codex" => "openai",
            other => other,
        };
        json!({"via": format!("cli:{cli}"), "provider": provider, "native_id": null, "model_source": "cli default"})
    } else {
        json!({"via": "self"})
    };
    value.as_object().cloned().unwrap_or_default()
}

fn with(mut base: Value, extra: Map<String, Value>) -> Value {
    if let Some(map) = base.as_object_mut() {
        for (k, v) in extra {
            map.insert(k, v);
        }
    }
    base
}

/// How to give this seat a fresh context from here, and on which model.
pub(crate) fn how(card: &Value, env: &Value, policy: &Value) -> Value {
    if let Some(card) = card.as_str() {
        // solo: "current model @ high"
        let effort = card
            .contains('@')
            .then(|| py::strip(card.rsplit('@').next().unwrap_or("")).to_string());
        return with(
            json!({"model": "current model", "effort": effort}),
            fresh(env),
        );
    }
    let name = card.s("model");
    let creator = card.at("creator");
    let effort = providers::effort_of(name);
    let out = json!({"model": name, "creator": creator, "effort": effort});
    let provider = providers::creator_provider(creator.as_str().unwrap_or(""));
    if provider == Some("claude") && env.s("fresh_contexts") == "subagents" {
        let alias = providers::claude_alias(name).unwrap_or("inherit");
        return with(
            out,
            json!({"via": "subagent", "alias": alias})
                .as_object()
                .cloned()
                .unwrap_or_default(),
        );
    }
    let cli = match provider {
        Some("claude") => Some("claude"),
        Some("openai") => Some("codex"),
        Some("gemini") => Some("gemini"),
        _ => None,
    };
    if let (Some(cli), Some(provider)) = (cli, provider) {
        if env.at("cli").truthy(cli) {
            let (id, source) = providers::native_id(provider, Some(name), policy);
            let extra = json!({"via": format!("cli:{cli}"), "provider": provider, "native_id": id, "model_source": source});
            return with(out, extra.as_object().cloned().unwrap_or_default());
        }
    }
    if env.truthy("openrouter_key") && card.truthy("openrouter_id") {
        let extra = json!({"via": "openrouter", "provider": "openrouter", "native_id": card.at("openrouter_id"), "model_source": "openrouter id"});
        return with(out, extra.as_object().cloned().unwrap_or_default());
    }
    let effort = effort.unwrap_or_else(|| "high".to_string());
    with(
        json!({"model": "stand-in: the chair's model in a fresh context", "effort": effort, "stand_in_for": name}),
        fresh(env),
    )
}

/// Can this seat open web pages? yes (a Claude subagent or the claude CLI get the web tools),
/// no (a bare API call), or unknown (it depends on how that CLI is set up).
pub(crate) fn web_access(seat: &Value) -> &'static str {
    match seat.s("via") {
        "subagent" | "cli:claude" => "yes",
        "openrouter" => "no",
        _ => "unknown",
    }
}

/// The run's settings for its support members (pinned at init).
pub(crate) fn sup(run: &Value) -> Value {
    if run.truthy("support") {
        run.at("support").clone()
    } else {
        core::members(&core::project_root(), &core::MemberFlags::default())
    }
}

/// Models for the researchers: a route that can search the web first, then as many labs as there are.
fn research_cards(
    chosen: &[Value],
    env: &Value,
    policy: &Value,
    n: usize,
    model: &str,
    chair: &Value,
) -> Vec<Value> {
    if model == "chair" && !chair.is_null() {
        return vec![chair.clone(); n];
    }
    let Some(first) = chosen.first() else {
        return Vec::new();
    };
    if first.is_string() {
        return vec![first.clone(); n];
    }
    let mut uniq: Vec<&Value> = Vec::new();
    for c in chosen {
        if !uniq.iter().any(|u| u.at("model") == c.at("model")) {
            uniq.push(c);
        }
    }
    let seat: Vec<Value> = uniq.iter().map(|c| how(c, env, policy)).collect();
    let web: Vec<&str> = seat.iter().map(web_access).collect();
    let web_rank = |w: &str| {
        ["yes", "unknown", "no"]
            .iter()
            .position(|x| *x == w)
            .unwrap_or(2)
    };
    // a model that can really be called, then one that can open web pages, then the stronger one
    let mut order: Vec<usize> = (0..uniq.len()).collect();
    order.sort_by(|&a, &b| {
        let key = |i: usize| (seat[i].truthy("stand_in_for"), web_rank(web[i]));
        let quality = |i: usize| uniq[i].num("quality").unwrap_or(0.0);
        key(a).cmp(&key(b)).then(
            quality(b)
                .partial_cmp(&quality(a))
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });
    let mut usable: Vec<usize> = order
        .iter()
        .copied()
        .filter(|&i| web[i] != "no" && !seat[i].truthy("stand_in_for"))
        .collect();
    if usable.is_empty() {
        usable = order.iter().copied().take(1).collect();
    }
    let mut picked: Vec<usize> = vec![usable[0]];
    while picked.len() < n {
        // a lab not yet seated, then a model not yet seated, then again from the top
        let labs: Vec<&Value> = picked.iter().map(|&i| uniq[i].at("creator")).collect();
        let next = usable
            .iter()
            .copied()
            .find(|&i| !labs.contains(&uniq[i].at("creator")))
            .or_else(|| usable.iter().copied().find(|i| !picked.contains(i)))
            .unwrap_or(usable[picked.len() % usable.len()]);
        picked.push(next);
    }
    picked
        .into_iter()
        .take(n)
        .map(|i| uniq[i].clone())
        .collect()
}

/// Seat every role of a council of this tier: experts, the red team, elders, the control arm, and
/// the support members when the run has them.
pub(crate) fn assign_seats(
    tier: &str,
    picks: &Value,
    env: &Value,
    policy: &Value,
    support: Option<&Value>,
) -> Value {
    let Some(shape) = core::shape(tier) else {
        return json!({});
    };
    let n = shape.experts;
    let experts = picks.at("experts");
    let chosen: Vec<Value> = match experts {
        Value::String(_) => vec![experts.clone(); n],
        Value::Array(list) if n >= list.len() => list.clone(),
        Value::Array(list) => {
            // lite: prefer one of each distinct model
            let mut chosen: Vec<Value> = Vec::new();
            for c in list {
                if !chosen.iter().any(|x| x.at("model") == c.at("model")) {
                    chosen.push(c.clone());
                }
            }
            let rest: Vec<Value> = list
                .iter()
                .filter(|c| !chosen.contains(c))
                .cloned()
                .collect();
            chosen.extend(rest);
            chosen.truncate(n);
            while chosen.len() < n {
                chosen.push(list[0].clone());
            }
            chosen
        }
        _ => Vec::new(),
    };
    let seated = |role: &str, card: &Value| {
        with(
            json!({"role": role}),
            how(card, env, policy)
                .as_object()
                .cloned()
                .unwrap_or_default(),
        )
    };
    let mut seats = Map::new();
    for (i, c) in chosen.iter().enumerate() {
        seats.insert(format!("L{}", i + 1), seated("expert", c));
    }
    seats.insert("RT".to_string(), seated("red_team", picks.at("red_team")));
    let chair = picks.at("chair");
    let elders: Vec<Value> = match picks.at("elders") {
        e @ Value::String(_) => vec![e.clone(); shape.elders],
        Value::Array(list) if shape.elders < list.len() => {
            // lite: prefer another lab, then another model, than the chair's
            let chair_model = providers::base_name(chair.s("model"));
            let mut sorted = list.clone();
            sorted.sort_by_key(|e| {
                (
                    e.at("creator") == chair.at("creator"),
                    providers::base_name(e.s("model")) == chair_model,
                )
            });
            sorted.truncate(shape.elders);
            sorted
        }
        Value::Array(list) => list.clone(),
        _ => Vec::new(),
    };
    for (i, e) in elders.iter().take(shape.elders).enumerate() {
        let letter = (b'A' + i as u8) as char;
        seats.insert(format!("E{letter}"), seated("elder", e));
    }
    seats.insert("CTRL".to_string(), seated("control", chair));
    if let Some(support) = support.filter(|s| py::truthy(s)) {
        let beats = core::beats_for(support);
        let cards = if beats.is_empty() {
            Vec::new()
        } else {
            research_cards(
                &chosen,
                env,
                policy,
                beats.len(),
                support.at("research").s("model"),
                chair,
            )
        };
        for ((sid, (name, text)), card) in core::RESEARCH_IDS.iter().zip(beats).zip(cards) {
            let mut seat = with(
                json!({"role": "researcher", "beat": name, "beat_text": text}),
                how(&card, env, policy)
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
            );
            seat["web"] = json!(web_access(&seat));
            seats.insert((*sid).to_string(), seat);
        }
        if support.at("map").s("when") != "never" {
            // the map is kept: by default the strongest reader writes it
            let card = if support.at("map").s("model") == "chair" {
                chair.clone()
            } else {
                chosen.first().cloned().unwrap_or(Value::Null)
            };
            seats.insert("MAP".to_string(), seated("mapper", &card));
        }
    }
    Value::Object(seats)
}

/// Every seat and its model, on one line, with stand-ins labelled.
pub(crate) fn seat_line(run: &Value) -> String {
    let mut seats: Vec<(&String, &Value)> = items(run.at("seats")).collect();
    seats.sort_by(|a, b| a.0.cmp(b.0));
    seats
        .into_iter()
        .map(|(s, v)| {
            if v.truthy("stand_in_for") {
                format!("{s}=STAND-IN for {}", v.show("stand_in_for"))
            } else if v.s("model_source") == "cli default" {
                format!("{s}={} (ran on the CLI's default model)", v.show("model"))
            } else {
                format!("{s}={}", v.show("model"))
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}
