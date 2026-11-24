//! Background jobs: deadline reminders, new announcements, grade updates and an
//! optional daily AI briefing. Runs while the app is open (including in the tray).
use crate::{canvas, codex::CodexState, config, db::Db, AppState};
use chrono::{Local, NaiveTime, TimeZone};
use serde_json::{json, Value};
use std::{
    sync::{mpsc, Arc},
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

pub enum Signal {
    Wake,
}

pub fn notify(app: &AppHandle, title: &str, body: &str) {
    let Ok(dir) = crate::codex::data_dir(app) else { return };
    if !config::load(&dir).notifications {
        return;
    }
    let _ = app.notification().builder().title(title).body(body).show();
}

fn parse_time(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&chrono::Utc))
}

fn local_label(t: chrono::DateTime<chrono::Utc>) -> String {
    t.with_timezone(&Local).format("%m/%d(%a) %H:%M").to_string()
}

/// Also true for assignments whose status Canvas cannot know (paper, external tool): no reminder
/// is better than a wrong "not submitted" alert.
fn is_submitted(a: &Value) -> bool {
    canvas::is_submitted(a) || canvas::submission_unknown(a)
}

/// Collects (assignment, course name) pairs with due dates from to-do and upcoming events.
fn upcoming_assignments(client: &canvas::Client) -> Result<Vec<(Value, String)>, String> {
    let up = client.upcoming()?;
    let mut out: Vec<(Value, String)> = Vec::new();
    for list in ["todo", "upcoming_events"] {
        for item in up.get(list).and_then(Value::as_array).into_iter().flatten() {
            if let Some(a) = item.get("assignment") {
                let id = a.get("id").map(Value::to_string);
                if out.iter().any(|(x, _)| x.get("id").map(Value::to_string) == id) {
                    continue;
                }
                let course = item.get("context_name").and_then(Value::as_str).unwrap_or("").to_string();
                out.push((a.clone(), course));
            }
        }
    }
    Ok(out)
}

fn deadline_alert(app: &AppHandle, db: &Db, client: &canvas::Client, params: &Value) -> Result<String, String> {
    let mut hours: Vec<i64> = params.get("hours_before").and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_i64).filter(|h| (1..=168).contains(h)).collect())
        .unwrap_or_else(|| vec![24, 3]);
    hours.sort_unstable();
    let now = chrono::Utc::now();
    let mut sent = 0;
    let assignments = upcoming_assignments(client)?;
    for (a, course) in &assignments {
        if is_submitted(a) {
            continue;
        }
        let Some(due) = a.get("due_at").and_then(Value::as_str).and_then(parse_time) else { continue };
        let left = due - now;
        if left <= chrono::Duration::zero() {
            continue;
        }
        // Notify once, for the tightest threshold that has been crossed.
        if let Some(h) = hours.iter().find(|h| left <= chrono::Duration::hours(**h)) {
            let id = a.get("id").map(|v| v.to_string()).unwrap_or_default();
            let key = format!("deadline:{id}:{}:{h}", due.timestamp());
            if db.first_time(&key) {
                let name = a.get("name").and_then(Value::as_str).unwrap_or("과제");
                let remain = if left.num_hours() >= 1 { format!("{}시간", left.num_hours()) } else { format!("{}분", left.num_minutes().max(1)) };
                notify(app, &format!("⏰ 마감 {remain} 전: {name}"), &format!("{course} · {} 마감", local_label(due)));
                sent += 1;
            }
        }
    }
    Ok(format!("미제출 과제 {}개 확인, 알림 {sent}개", assignments.iter().filter(|(a, _)| !is_submitted(a)).count()))
}

fn new_announcements(app: &AppHandle, db: &Db, client: &canvas::Client, seed: bool) -> Result<String, String> {
    let courses = client.courses(false)?;
    let names: std::collections::HashMap<String, String> = courses.iter()
        .filter_map(|c| Some((format!("course_{}", c.get("id")?.as_str().map(str::to_string).unwrap_or_else(|| c["id"].to_string())), c.get("name")?.as_str()?.to_string())))
        .collect();
    let ids: Vec<u64> = courses.iter().filter_map(|c| canvas::require_id(c, "id").ok()).collect();
    let list = client.announcements(&ids, 7)?;
    let mut fresh = Vec::new();
    for a in &list {
        let key = format!("ann:{}", a.get("id").map(Value::to_string).unwrap_or_default());
        if db.first_time(&key) && !seed {
            fresh.push(a);
        }
    }
    for a in fresh.iter().take(5) {
        let course = a.get("context_code").and_then(Value::as_str).and_then(|c| names.get(c)).cloned().unwrap_or_default();
        let title = a.get("title").and_then(Value::as_str).unwrap_or("새 공지");
        let preview: String = a.get("message").and_then(Value::as_str).unwrap_or("").chars().take(120).collect();
        notify(app, &format!("📢 {course}: {title}"), &preview);
    }
    if fresh.len() > 5 {
        notify(app, "📢 새 공지", &format!("외 {}개의 새 공지가 있습니다.", fresh.len() - 5));
    }
    if !fresh.is_empty() {
        let _ = app.emit("canvas-updated", json!({"kind": "announcements"}));
    }
    Ok(if seed { format!("공지 {}개 기준 설정", list.len()) } else { format!("새 공지 {}개", fresh.len()) })
}

fn grade_updates(app: &AppHandle, db: &Db, client: &canvas::Client, seed: bool) -> Result<String, String> {
    let courses = client.courses(false)?;
    let mut fresh = Vec::new();
    for c in &courses {
        let Ok(id) = canvas::require_id(c, "id") else { continue };
        let course = c.get("name").and_then(Value::as_str).unwrap_or("").to_string();
        // A course without student access returns an error; skip it quietly.
        let Ok(subs) = client.submissions(id) else { continue };
        for s in subs {
            let Some(graded) = s.get("graded_at").and_then(Value::as_str) else { continue };
            if s.get("score").map_or(true, Value::is_null) && s.get("grade").map_or(true, Value::is_null) {
                continue;
            }
            let key = format!("grade:{}:{}:{graded}", id, s.get("assignment_id").map(Value::to_string).unwrap_or_default());
            if db.first_time(&key) && !seed {
                fresh.push((course.clone(), s));
            }
        }
    }
    for (course, s) in fresh.iter().take(5) {
        let name = s.get("assignment_name").and_then(Value::as_str).unwrap_or("과제");
        let score = s.get("score").filter(|v| !v.is_null()).map(|v| v.to_string()).or_else(|| s.get("grade").and_then(Value::as_str).map(str::to_string)).unwrap_or_default();
        let total = s.get("points_possible").filter(|v| !v.is_null()).map(|v| format!(" / {v}")).unwrap_or_default();
        notify(app, &format!("📝 성적 등록: {name}"), &format!("{course} · {score}{total}"));
    }
    if fresh.len() > 5 {
        notify(app, "📝 성적 등록", &format!("외 {}개의 성적이 새로 등록되었습니다.", fresh.len() - 5));
    }
    Ok(if seed { "성적 기준 설정".into() } else { format!("새 성적 {}개", fresh.len()) })
}

fn briefing_due(params: &Value, last_run: Option<i64>) -> bool {
    let time = params.get("time").and_then(Value::as_str).and_then(|t| NaiveTime::parse_from_str(t, "%H:%M").ok())
        .unwrap_or_else(|| NaiveTime::from_hms_opt(8, 0, 0).unwrap());
    let now = Local::now();
    let Some(today_at) = Local.from_local_datetime(&now.date_naive().and_time(time)).earliest() else { return false };
    now >= today_at && last_run.map_or(true, |t| t < today_at.timestamp())
}

fn ai_briefing(app: &AppHandle, db: &Db, codex: &CodexState, params: &Value) -> Result<String, String> {
    let prompt = params.get("prompt").and_then(Value::as_str).filter(|p| !p.trim().is_empty())
        .unwrap_or("이번 주 마감 과제와 최근 공지를 확인해서 오늘 해야 할 일을 우선순위대로 짧게 정리해줘.");
    let text = codex.run_prompt(app, prompt, Duration::from_secs(300))?;
    db.add_briefing(&text);
    let preview: String = text.lines().filter(|l| !l.trim().is_empty()).take(3).collect::<Vec<_>>().join(" ").chars().take(180).collect();
    notify(app, "☀️ 오늘의 브리핑", &preview);
    let _ = app.emit("briefing-updated", ());
    Ok("브리핑 생성".into())
}

fn run_due_jobs(app: &AppHandle, state: &AppState) {
    // The scheduler uses its own connection so slow network work never blocks UI commands.
    let Ok(db) = Db::open(&state.data_dir) else { return };
    let jobs = db.jobs().unwrap_or_default();
    let now = chrono::Utc::now().timestamp();
    let client = config::canvas_client(&state.data_dir);
    for job in jobs {
        if job["enabled"] != true {
            continue;
        }
        let kind = job["kind"].as_str().unwrap_or("").to_string();
        let last = job["lastRun"].as_i64();
        let interval = job["intervalMin"].as_i64().unwrap_or(60) * 60;
        let due = if kind == "ai_briefing" { briefing_due(&job["params"], last) } else { last.map_or(true, |t| now - t >= interval) };
        if !due {
            continue;
        }
        // The first successful run only records what already exists, so connecting
        // Canvas does not flood the user with old announcements and grades.
        let seed_key = format!("seeded:{kind}");
        let seed = !db.seen(&seed_key);
        let result = match (kind.as_str(), &client) {
            ("ai_briefing", _) => ai_briefing(app, &db, &state.codex, &job["params"]),
            (_, Err(e)) => Err(e.clone()),
            ("deadline_alert", Ok(c)) => deadline_alert(app, &db, c, &job["params"]),
            ("new_announcements", Ok(c)) => new_announcements(app, &db, c, seed),
            ("grade_updates", Ok(c)) => grade_updates(app, &db, c, seed),
            _ => Err("unknown job".into()),
        };
        if result.is_ok() {
            db.first_time(&seed_key);
        }
        let summary = match &result {
            Ok(s) => s.clone(),
            Err(e) => format!("오류: {e}"),
        };
        db.mark_job_run(&kind, &summary);
        let _ = app.emit("jobs-updated", ());
    }
    db.prune_notified(120);
}

/// Starts the scheduler loop and the approval-queue watcher.
pub fn spawn(app: AppHandle) -> mpsc::Sender<Signal> {
    let (tx, rx) = mpsc::channel::<Signal>();
    let job_app = app.clone();
    std::thread::spawn(move || {
        // Give the UI and network a moment after login/boot.
        let _ = rx.recv_timeout(Duration::from_secs(20));
        loop {
            let state = job_app.state::<Arc<AppState>>();
            run_due_jobs(&job_app, &state);
            let _ = rx.recv_timeout(Duration::from_secs(60));
        }
    });
    // Watches for write proposals added by the Canvas MCP helper process.
    std::thread::spawn(move || {
        let mut last = -1i64;
        loop {
            let count = {
                let state = app.state::<Arc<AppState>>();
                let count = state.db.lock().map(|db| db.pending_count()).unwrap_or(0);
                count
            };
            if count != last {
                if last >= 0 && count > last {
                    notify(&app, "✋ 승인이 필요한 요청", "AI가 Canvas 변경을 제안했습니다. 앱의 '승인 대기'에서 확인하세요.");
                }
                let _ = app.emit("pending-changed", json!({"count": count}));
                last = count;
            }
            std::thread::sleep(Duration::from_secs(3));
        }
    });
    tx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submission_states() {
        assert!(is_submitted(&json!({"submission": {"workflow_state": "submitted"}})));
        assert!(is_submitted(&json!({"submission": {"workflow_state": "unsubmitted", "excused": true}})));
        assert!(!is_submitted(&json!({"submission": {"workflow_state": "unsubmitted"}})));
        assert!(!is_submitted(&json!({})));
    }

    #[test]
    fn briefing_runs_once_per_day() {
        let early = json!({"time": "00:00"});
        assert!(briefing_due(&early, None));
        assert!(!briefing_due(&early, Some(chrono::Utc::now().timestamp())));
        let late = json!({"time": "23:59"});
        let now = Local::now();
        if now.format("%H:%M").to_string() != "23:59" {
            assert!(!briefing_due(&late, None));
        }
    }
}
