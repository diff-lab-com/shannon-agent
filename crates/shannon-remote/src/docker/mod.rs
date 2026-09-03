//! Docker execution world: run commands and file ops inside a running
//! container via the `docker` CLI (`docker exec`), locally or over an SSH
//! hop (`ssh_target`).

pub mod fs;
pub mod process;

pub use fs::DockerExecFs;
pub use process::{compose_docker_exec, ContainerInfo, DockerExecProcess, list_running_containers};
