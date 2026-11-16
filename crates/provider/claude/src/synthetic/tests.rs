use super::*;

#[test]
fn task_notifications_become_tool_rows() {
    let text = "<task-notification>\n<task-id>b7ktc4esc</task-id>\n<tool-use-id>toolu_1</tool-use-id>\n<output-file>/tmp/b7.output</output-file>\n<status>completed</status>\n<summary>Background command \"Wait for CI\" completed (exit code 0)</summary>\n</task-notification>";
    let notification = TaskNotification::parse(text).unwrap();
    assert_eq!(notification.task_id.as_deref(), Some("b7ktc4esc"));
    assert_eq!(notification.tool_use_id.as_deref(), Some("toolu_1"));
    let item = notification.tool_call("task_notification_x");
    assert_eq!(item["status"], "completed");
    assert_eq!(
        item["detail"]["label"],
        "Background command \"Wait for CI\" completed (exit code 0)"
    );
    assert_eq!(item["metadata"]["outputFile"], "/tmp/b7.output");

    let failed =
        TaskNotification::parse("<task-notification><status>failed</status></task-notification>")
            .unwrap()
            .tool_call("id");
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["detail"]["label"], "Background task failed");
    assert_eq!(TaskNotification::parse("plain prompt"), None);
}

#[test]
fn transcript_noise_is_hidden_and_commands_read_back() {
    for noise in [
        "[Request interrupted by user]",
        "[Request interrupted by user for tool use]",
        "No response requested.",
        "<local-command-stdout>Set model to opus</local-command-stdout>",
        "  ",
    ] {
        assert_eq!(user_text(noise), None, "{noise}");
    }
    assert_eq!(
        user_text("<command-message>review</command-message>\n<command-name>/review</command-name>\n<command-args>#12</command-args>").as_deref(),
        Some("/review #12")
    );
    assert_eq!(
        user_text("<command-name>clear</command-name>").as_deref(),
        Some("/clear")
    );
    assert_eq!(user_text(" hello ").as_deref(), Some(" hello "));
}

#[test]
fn prompts_that_mention_transcript_markers_stay_user_text() {
    let quoted = "what does <command-name> do?";
    assert_eq!(user_text(quoted).as_deref(), Some(quoted));
    assert_eq!(
        TaskNotification::parse("explain <task-notification><status>failed</status>"),
        None
    );
    assert!(
        TaskNotification::parse("\n<task-notification><status>failed</status></task-notification>")
            .is_some()
    );
}
