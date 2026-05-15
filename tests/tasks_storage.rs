use heddle::tasks::storage::{
    create_task, format_tasks_summary, get_tasks_path, load_tasks, save_tasks, update_task,
    TaskUpdate,
};
use heddle::tasks::types::{Task, TaskStatus};

mod common;
use common::Sandbox;

#[test]
fn tasks_path_under_project() {
    let _sb = Sandbox::new("tasks-path");
    let p = get_tasks_path(Some("/some/project"));
    let s = p.to_string_lossy();
    assert!(s.contains("tasks.json"));
    assert!(s.contains("projects"));
}

#[test]
fn load_returns_empty_when_no_file() {
    let _sb = Sandbox::new("tasks-loadempty");
    let tasks = load_tasks(Some("/nonexistent/project"));
    assert!(tasks.is_empty());
}

#[test]
fn create_adds_task_with_fields() {
    let _sb = Sandbox::new("tasks-create");
    let project_path = "/test/create-project";
    let task = create_task("Write tests", "session-1", Some(project_path), None).unwrap();
    assert!(!task.id.is_empty());
    assert_eq!(task.title, "Write tests");
    assert_eq!(task.status, TaskStatus::Pending);
    assert_eq!(task.session_id, "session-1");
    assert!(task.created.starts_with("20"));
    assert_eq!(task.updated, task.created);
    assert!(task.details.is_none());

    let loaded = load_tasks(Some(project_path));
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].title, "Write tests");
}

#[test]
fn create_with_details_stores_them() {
    let _sb = Sandbox::new("tasks-details");
    let task = create_task(
        "Detailed task",
        "session-2",
        Some("/test/details-project"),
        Some("Some extra details"),
    )
    .unwrap();
    assert_eq!(task.details.as_deref(), Some("Some extra details"));
}

#[test]
fn save_load_roundtrip() {
    let _sb = Sandbox::new("tasks-rt");
    let project_path = "/test/roundtrip-project";
    let tasks = vec![
        Task {
            id: "abc-123".into(),
            title: "Task A".into(),
            status: TaskStatus::Pending,
            created: "2026-01-01T00:00:00.000Z".into(),
            updated: "2026-01-01T00:00:00.000Z".into(),
            session_id: "s1".into(),
            details: None,
        },
        Task {
            id: "def-456".into(),
            title: "Task B".into(),
            status: TaskStatus::Done,
            created: "2026-01-02T00:00:00.000Z".into(),
            updated: "2026-01-02T00:00:00.000Z".into(),
            session_id: "s2".into(),
            details: Some("Completed".into()),
        },
    ];
    save_tasks(&tasks, Some(project_path)).unwrap();
    let loaded = load_tasks(Some(project_path));
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].title, "Task A");
    assert_eq!(loaded[1].details.as_deref(), Some("Completed"));
}

#[test]
fn save_writes_pretty_printed_json() {
    let _sb = Sandbox::new("tasks-pretty");
    let project_path = "/test/pretty-project";
    save_tasks(
        &[Task {
            id: "x".into(),
            title: "T".into(),
            status: TaskStatus::Pending,
            created: "2026-01-01T00:00:00.000Z".into(),
            updated: "2026-01-01T00:00:00.000Z".into(),
            session_id: "s".into(),
            details: None,
        }],
        Some(project_path),
    )
    .unwrap();
    let raw = std::fs::read_to_string(get_tasks_path(Some(project_path))).unwrap();
    assert!(raw.contains('\n'));
    assert!(raw.lines().count() > 2);
}

#[test]
fn update_modifies_task() {
    let _sb = Sandbox::new("tasks-update");
    let project_path = "/test/update-project";
    let task = create_task("Original title", "session-1", Some(project_path), None).unwrap();
    let updated = update_task(
        &task.id,
        TaskUpdate {
            status: Some(TaskStatus::InProgress),
            title: Some("New title".into()),
            details: None,
        },
        Some(project_path),
    )
    .unwrap();
    assert_eq!(updated.status, TaskStatus::InProgress);
    assert_eq!(updated.title, "New title");
    assert!(updated.updated >= task.created);
    let loaded = load_tasks(Some(project_path));
    assert_eq!(loaded[0].status, TaskStatus::InProgress);
    assert_eq!(loaded[0].title, "New title");
}

#[test]
fn update_with_details_adds_them() {
    let _sb = Sandbox::new("tasks-upd-det");
    let project_path = "/test/update-details-project";
    let task = create_task("Task", "session-1", Some(project_path), None).unwrap();
    let updated = update_task(
        &task.id,
        TaskUpdate {
            status: None,
            title: None,
            details: Some("Added details".into()),
        },
        Some(project_path),
    )
    .unwrap();
    assert_eq!(updated.details.as_deref(), Some("Added details"));
}

#[test]
fn update_nonexistent_errors() {
    let _sb = Sandbox::new("tasks-upd-none");
    let project_path = "/test/nonexistent-update";
    let r = update_task(
        "nonexistent-id",
        TaskUpdate {
            status: Some(TaskStatus::Done),
            title: None,
            details: None,
        },
        Some(project_path),
    );
    assert!(r.is_err());
}

#[test]
fn format_summary_groups_by_status() {
    let _sb = Sandbox::new("tasks-fmt-group");
    let tasks = vec![
        Task {
            id: "1".into(),
            title: "Pending task".into(),
            status: TaskStatus::Pending,
            created: "2026-01-01T00:00:00.000Z".into(),
            updated: "2026-01-01T00:00:00.000Z".into(),
            session_id: "current".into(),
            details: None,
        },
        Task {
            id: "2".into(),
            title: "Done task".into(),
            status: TaskStatus::Done,
            created: "2026-01-01T00:00:00.000Z".into(),
            updated: "2026-01-01T00:00:00.000Z".into(),
            session_id: "current".into(),
            details: None,
        },
        Task {
            id: "3".into(),
            title: "In progress task".into(),
            status: TaskStatus::InProgress,
            created: "2026-01-01T00:00:00.000Z".into(),
            updated: "2026-01-01T00:00:00.000Z".into(),
            session_id: "current".into(),
            details: None,
        },
    ];
    let summary = format_tasks_summary(&tasks, "current");
    assert!(summary.contains("Pending task"));
    assert!(summary.contains("Done task"));
    assert!(summary.contains("In progress task"));
}

#[test]
fn format_summary_flags_stale() {
    let _sb = Sandbox::new("tasks-fmt-stale");
    let tasks = vec![
        Task {
            id: "1".into(),
            title: "Current session task".into(),
            status: TaskStatus::Pending,
            created: "2026-01-01T00:00:00.000Z".into(),
            updated: "2026-01-01T00:00:00.000Z".into(),
            session_id: "current-session".into(),
            details: None,
        },
        Task {
            id: "2".into(),
            title: "Old session task".into(),
            status: TaskStatus::InProgress,
            created: "2026-01-01T00:00:00.000Z".into(),
            updated: "2026-01-01T00:00:00.000Z".into(),
            session_id: "old-session".into(),
            details: None,
        },
    ];
    let summary = format_tasks_summary(&tasks, "current-session");
    // The stale flag attaches to the "Old session task" line, never the current
    let stale_line = summary
        .lines()
        .find(|l| l.contains("stale"))
        .expect("expected a stale line");
    assert!(stale_line.contains("Old session task"));
    assert!(!summary
        .lines()
        .filter(|l| l.contains("Current session task"))
        .any(|l| l.contains("stale")));
}

#[test]
fn format_summary_empty_message() {
    let _sb = Sandbox::new("tasks-fmt-empty");
    let summary = format_tasks_summary(&[], "session-1");
    assert!(summary.contains("No tasks"));
}
