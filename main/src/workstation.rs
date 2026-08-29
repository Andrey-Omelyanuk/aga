use crate::agent::Executor;
use crate::cluster::Cluster;

/// Разрешить способ исполнения команд для воркстейшна.
///
/// Воркстейшн — под в Kubernetes (`ws-<id>`) или контейнер в docker (dev,
/// `AGA_WS_BACKEND=docker`), команды агента выполняются внутри него через
/// `kubectl exec` / `docker exec`. Без воркстейшна — локальный `sh -c`
/// (dev-режим, legacy `/tasks/:role`).
pub fn executor_for_workstation(ws_id: Option<i64>, cluster: &Cluster) -> Executor {
    match ws_id {
        Some(id) => {
            let name = Cluster::pod_name(id);
            match cluster.backend {
                crate::cluster::Backend::K8s => Executor::KubectlExec {
                    namespace: cluster.namespace.clone(),
                    pod: name,
                },
                crate::cluster::Backend::Docker => Executor::DockerExec { container: name },
            }
        }
        None => Executor::Sh,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cluster(backend: crate::cluster::Backend) -> Cluster {
        Cluster {
            backend,
            kubectl: "kubectl".into(),
            namespace: "aga".into(),
            template: "/nonexistent/workstation-pod.yaml".into(),
            image: "aga-workstation:test".into(),
            wait_timeout_secs: 1,
        }
    }

    #[test]
    fn workstation_executor_targets_its_pod() {
        match executor_for_workstation(Some(5), &cluster(crate::cluster::Backend::K8s)) {
            Executor::KubectlExec { namespace, pod } => {
                assert_eq!(namespace, "aga");
                assert_eq!(pod, "ws-5");
            }
            _ => panic!("expected kubectl exec"),
        }
    }

    #[test]
    fn docker_backend_executor_targets_its_container() {
        match executor_for_workstation(Some(6), &cluster(crate::cluster::Backend::Docker)) {
            Executor::DockerExec { container } => {
                assert_eq!(container, "ws-6");
            }
            _ => panic!("expected docker exec"),
        }
    }

    #[test]
    fn without_workstation_executor_is_local_sh() {
        assert!(matches!(
            executor_for_workstation(None, &cluster(crate::cluster::Backend::K8s)),
            Executor::Sh
        ));
    }
}
