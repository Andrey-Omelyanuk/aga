use crate::agent::Executor;
use crate::cluster::Cluster;

/// Разрешить способ исполнения команд для воркстейшна.
///
/// Воркстейшн — под в Kubernetes (`ws-<id>`), команды агента выполняются
/// внутри пода через `kubectl exec`. Без воркстейшна — локальный `sh -c`
/// (dev-режим, legacy `/tasks/:role`).
pub fn executor_for_workstation(ws_id: Option<i64>, namespace: &str) -> Executor {
    match ws_id {
        Some(id) => Executor::KubectlExec {
            namespace: namespace.to_string(),
            pod: Cluster::pod_name(id),
        },
        None => Executor::Sh,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workstation_executor_targets_its_pod() {
        match executor_for_workstation(Some(5), "aga") {
            Executor::KubectlExec { namespace, pod } => {
                assert_eq!(namespace, "aga");
                assert_eq!(pod, "ws-5");
            }
            _ => panic!("expected kubectl exec"),
        }
    }

    #[test]
    fn without_workstation_executor_is_local_sh() {
        assert!(matches!(
            executor_for_workstation(None, "aga"),
            Executor::Sh
        ));
    }
}
