use std::{
    error::Error,
    sync::{LazyLock, Mutex},
};
use tokio::task::JoinHandle;

static TASKS: LazyLock<Mutex<Vec<JoinHandle<()>>>> = LazyLock::new(|| Mutex::new(vec![]));

pub fn add_task(task: tokio::task::JoinHandle<()>) -> Result<(), Box<dyn Error>> {
    TASKS.lock()?.push(task);
    Ok(())
}

pub fn get_tasks() -> Result<Vec<tokio::task::JoinHandle<()>>, Box<dyn Error>> {
    Ok(TASKS.lock()?.drain(..).collect())
}
