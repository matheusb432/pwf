#![cfg(test)]

mod support;
mod project {
    mod add_project;
    mod get_project;
    mod list_projects;
    mod rename_project;
    mod resume_project;
    mod update_project;
}
mod note {
    mod add_note;
    mod edit_note;
    mod list_notes;
    mod remove_note;
}
mod settings {
    mod get_user_settings;
}
mod task {
    mod add_task;
    mod cancel_task;
    mod clone_task;
    mod complete_task;
    mod edit_task;
    mod get_task;
    mod get_task_dag;
    mod get_task_record;
    mod list_tasks;
    mod remove_task;
    mod reopen_task;
    mod session {
        mod dispatch_confirmed_session;
        mod plan_session;
    }
}
