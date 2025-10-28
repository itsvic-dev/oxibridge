use std::sync::{LazyLock, Mutex};
use tokio::task::JoinHandle;

static TASKS: LazyLock<Mutex<Vec<JoinHandle<()>>>> = LazyLock::new(|| Mutex::new(vec![]));

pub fn add_task(task: tokio::task::JoinHandle<()>) {
    TASKS.lock().unwrap().push(task);
}

pub fn get_tasks() -> Vec<tokio::task::JoinHandle<()>> {
    TASKS.lock().unwrap().drain(..).collect()
}
