use std::process::Output;
use std::time::{Duration, Instant};

use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// Способ запуска воркстейшнов: под в Kubernetes или контейнер в Docker (dev).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    K8s,
    Docker,
}

impl Backend {
    fn from_env() -> Self {
        match std::env::var("AGA_WS_BACKEND").as_deref() {
            Ok("docker") => Backend::Docker,
            _ => Backend::K8s,
        }
    }
}

/// Управление воркстейшнами: в Kubernetes — поды (kubectl), в dev — контейнеры
/// (docker). Кластером/контейнерами управляет только ядро; агент внутри
/// воркстейшна про него не знает.
#[derive(Debug, Clone)]
pub struct Cluster {
    pub backend: Backend,
    pub kubectl: String,
    pub namespace: String,
    pub template: String,
    pub image: String,
    pub wait_timeout_secs: u64,
}

#[derive(Debug, Error)]
pub enum ClusterError {
    #[error("kubectl failed: {0}")]
    Kubectl(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Дефолтный манифест воркстейшн-пода. Реальный файл живёт в
/// `infra/k8s/workstation-pod.yaml`; этот текст — такой же, чтобы модуль был
/// самодостаточен и тестируем без файловой системы.
const DEFAULT_TEMPLATE: &str = r#"apiVersion: v1
kind: Pod
metadata:
  name: {{POD_NAME}}
  labels:
    app: aga-workstation
spec:
  automountServiceAccountToken: false
  containers:
    - name: workstation
      image: {{IMAGE}}
      imagePullPolicy: IfNotPresent
      command: ["/entrypoint.sh"]
      readinessProbe:
        exec:
          command: ["sh", "-c", "test -d /work/project/.git"]
        initialDelaySeconds: 2
        periodSeconds: 3
      env:
        - name: GIT_URL
          value: "{{GIT_URL}}"
        - name: BRANCH
          value: "{{BRANCH}}"
      securityContext:
        privileged: true
      resources:
        requests:
          cpu: "500m"
          memory: "512Mi"
        limits:
          cpu: "1"
          memory: "1Gi"
  restartPolicy: OnFailure
"#;

fn env(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

impl Cluster {
    pub fn from_env() -> Self {
        let wait_timeout_secs = std::env::var("AGA_K8S_WAIT_TIMEOUT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(120);
        Self {
            backend: Backend::from_env(),
            kubectl: env("AGA_K8S_KUBECTL", "kubectl"),
            namespace: env("AGA_K8S_NAMESPACE", "default"),
            template: env("AGA_K8S_TEMPLATE", "./infra/k8s/workstation-pod.yaml"),
            image: env("AGA_K8S_IMAGE", "aga-workstation:latest"),
            wait_timeout_secs,
        }
    }

    /// Имя пода воркстейшна. Производное от id: стабильно и уникально.
    pub fn pod_name(ws_id: i64) -> String {
        format!("ws-{ws_id}")
    }

    /// Ветка, на которой воркстейшн работает в своём поде.
    pub fn branch_name(ws_id: i64) -> String {
        format!("ws-{ws_id}")
    }

    /// Аргументы `docker run` для контейнера воркстейшна. Абсолютный путь на
    /// хосте монтируется в `/work/project` (entrypoint клон пропускает, работа
    /// идёт с примонтированной копией); git-URL клонируется как в k8s
    /// (GIT_URL/BRANCH).
    pub fn docker_run_args(
        container: &str,
        image: &str,
        git_url: &str,
        branch: &str,
    ) -> Vec<String> {
        let mut args = vec![
            "run".to_string(),
            "-d".to_string(),
            "--name".to_string(),
            container.to_string(),
            "--privileged".to_string(),
        ];
        if git_url.starts_with('/') {
            args.extend(["-v".to_string(), format!("{git_url}:/work/project")]);
        } else {
            args.extend([
                "-e".to_string(),
                format!("GIT_URL={git_url}"),
                "-e".to_string(),
                format!("BRANCH={branch}"),
            ]);
        }
        args.extend([image.to_string(), "/entrypoint.sh".to_string()]);
        args
    }

    /// Собрать манифест воркстейшн-пода: под с собственным Docker (DinD),
    /// клоном проекта из git и работой на своей ветке. Опциональный
    /// `secret` — имя k8s-Secret из кластера, который монтируется в под
    /// (секреты для сторонних CLI: ssh-ключ и т.п.); без имени секрета нет.
    pub fn render_workstation_manifest(
        &self,
        pod_name: &str,
        git_url: &str,
        branch: &str,
        secret: Option<&str>,
    ) -> Result<String, ClusterError> {
        let template = std::fs::read_to_string(&self.template).unwrap_or_else(|_| {
            // Файл можно не класть рядом с бинарником — встроенный дефолт
            // совпадает с инфраструктурным шаблоном.
            DEFAULT_TEMPLATE.to_string()
        });
        let mut manifest = template
            .replace("{{POD_NAME}}", pod_name)
            .replace("{{GIT_URL}}", git_url)
            .replace("{{BRANCH}}", branch)
            .replace("{{IMAGE}}", &self.image);
        if let Some(secret) = secret {
            // Монтируем секрет как файлы в контейнер (read-only) — точки входа
            // стабильны в обоих шаблонах (встроенном и в `infra/k8s`).
            manifest = manifest.replace(
                "      securityContext:\n        privileged: true",
                &format!(
                    "      securityContext:\n        privileged: true\n\
                     \x20     volumeMounts:\n\
                     \x20       - name: {secret}\n\
                     \x20         mountPath: /etc/secrets/{secret}\n\
                     \x20         readOnly: true"
                ),
            );
            manifest = manifest.replace(
                "  restartPolicy: OnFailure",
                &format!(
                    "  volumes:\n\
                     \x20   - name: {secret}\n\
                     \x20     secret:\n\
                     \x20       secretName: {secret}\n  restartPolicy: OnFailure"
                ),
            );
        }
        Ok(manifest)
    }

    /// Создать воркстейшн (под в k8s или контейнер в docker по backend).
    pub async fn create_pod(
        &self,
        pod_name: &str,
        git_url: &str,
        branch: &str,
        secret: Option<&str>,
    ) -> Result<(), ClusterError> {
        match self.backend {
            Backend::Docker => self.create_container(pod_name, git_url, branch).await,
            Backend::K8s => self.create_k8s_pod(pod_name, git_url, branch, secret).await,
        }
    }

    /// Создать под воркстейшна в кластере (`kubectl apply -f -`).
    async fn create_k8s_pod(
        &self,
        pod_name: &str,
        git_url: &str,
        branch: &str,
        secret: Option<&str>,
    ) -> Result<(), ClusterError> {
        let manifest = self.render_workstation_manifest(pod_name, git_url, branch, secret)?;
        let mut child = Command::new(&self.kubectl)
            .args(["apply", "-f", "-"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(manifest.as_bytes()).await?;
        }
        check_output(child.wait_with_output().await?)?;
        Ok(())
    }

    /// Создать/переиспользовать контейнер воркстейшна (docker). Уже
    /// существующий (compose-стенд) — переиспользуем: запущенный — без
    /// действий, остановленный — стартуем.
    async fn create_container(
        &self,
        container: &str,
        git_url: &str,
        branch: &str,
    ) -> Result<(), ClusterError> {
        let inspect = Command::new("docker")
            .args(["inspect", "-f", "{{.State.Running}}", container])
            .output()
            .await?;
        if inspect.status.success() {
            if String::from_utf8_lossy(&inspect.stdout).trim() == "true" {
                return Ok(());
            }
            let output = Command::new("docker")
                .args(["start", container])
                .output()
                .await?;
            check_output(output)?;
            return Ok(());
        }
        let output = Command::new("docker")
            .args(Self::docker_run_args(
                container,
                &self.image,
                git_url,
                branch,
            ))
            .output()
            .await?;
        check_output(output)?;
        Ok(())
    }

    /// Манифест k8s-Secret с приватным SSH-ключом (data `id_ed25519`).
    /// Секрет монтируется в под воркстейшна через `workstations.secret`
    /// (см. `render_workstation_manifest`); entrypoint раскладывает ключ в
    /// `~/.ssh` до git-клона.
    pub fn render_ssh_secret_manifest(
        &self,
        name: &str,
        private_key: &str,
    ) -> Result<String, ClusterError> {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(private_key.as_bytes());
        Ok(format!(
            "apiVersion: v1\n\
             kind: Secret\n\
             metadata:\n\
             \x20 name: {name}\n\
             \x20 namespace: {ns}\n\
             type: Opaque\n\
             data:\n\
             \x20 id_ed25519: {b64}\n",
            ns = self.namespace,
        ))
    }

    /// Создать/обновить k8s-Secret с приватным SSH-ключом (`kubectl apply`,
    /// идемпотентно). Только для k8s-бэкенда.
    pub async fn ensure_ssh_secret(
        &self,
        name: &str,
        private_key: &str,
    ) -> Result<(), ClusterError> {
        let manifest = self.render_ssh_secret_manifest(name, private_key)?;
        let mut child = Command::new(&self.kubectl)
            .args(["apply", "-f", "-"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(manifest.as_bytes()).await?;
        }
        check_output(child.wait_with_output().await?)?;
        Ok(())
    }

    /// Положить приватный SSH-ключ в `~/.ssh` существующего контейнера
    /// воркстейшна (docker). Dev-стенд поднимает ws-контейнеры заранее —
    /// ядро переиспользует их, поэтому ключ инжектится при подъёме станции.
    pub async fn inject_ssh_key(
        &self,
        container: &str,
        private_key: &str,
    ) -> Result<(), ClusterError> {
        let mut child = Command::new("docker")
            .args([
                "exec",
                "-i",
                container,
                "sh",
                "-c",
                "mkdir -p ~/.ssh && \
                 cat > ~/.ssh/id_ed25519 && \
                 chmod 600 ~/.ssh/id_ed25519 && \
                 printf 'IdentitiesOnly yes\\nStrictHostKeyChecking accept-new\\n' > ~/.ssh/config",
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(private_key.as_bytes()).await?;
        }
        check_output(child.wait_with_output().await?)?;
        Ok(())
    }

    /// Удалить воркстейшн (под в k8s или контейнер в docker).
    pub async fn delete_pod(&self, pod_name: &str) -> Result<(), ClusterError> {
        match self.backend {
            Backend::Docker => {
                let output = Command::new("docker")
                    .args(["rm", "-f", pod_name])
                    .output()
                    .await?;
                check_output(output)?;
                Ok(())
            }
            Backend::K8s => {
                let output = Command::new(&self.kubectl)
                    .args([
                        "delete",
                        "pod",
                        pod_name,
                        "-n",
                        &self.namespace,
                        "--ignore-not-found",
                    ])
                    .output()
                    .await?;
                check_output(output)?;
                Ok(())
            }
        }
    }

    /// Ждать готовности воркстейшна: под — Ready (проект склонирован,
    /// readinessProbe в манифесте), контейнер — `test -d /work/project/.git`.
    /// Возвращает false по таймауту или при падении (образ ещё тянется /
    /// контейнер не готов).
    pub async fn wait_ready(&self, pod_name: &str) -> Result<bool, ClusterError> {
        match self.backend {
            Backend::Docker => self.wait_container_ready(pod_name).await,
            Backend::K8s => self.wait_k8s_pod_ready(pod_name).await,
        }
    }

    async fn wait_container_ready(&self, container: &str) -> Result<bool, ClusterError> {
        let deadline = Instant::now() + Duration::from_secs(self.wait_timeout_secs);
        loop {
            let output = Command::new("docker")
                .args(["exec", container, "sh", "-c", "test -d /work/project/.git"])
                .output()
                .await?;
            if output.status.success() {
                return Ok(true);
            }
            if Instant::now() > deadline {
                return Ok(false);
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    async fn wait_k8s_pod_ready(&self, pod_name: &str) -> Result<bool, ClusterError> {
        let deadline = Instant::now() + Duration::from_secs(self.wait_timeout_secs);
        loop {
            let output = Command::new(&self.kubectl)
                .args([
                    "get",
                    "pod",
                    pod_name,
                    "-n",
                    &self.namespace,
                    "-o",
                    "jsonpath={.status.conditions[?(@.type==\"Ready\")].status}",
                ])
                .output()
                .await?;
            let ready = String::from_utf8_lossy(&output.stdout).trim().to_string();
            match ready.as_str() {
                "True" => return Ok(true),
                "False" => {
                    if Instant::now() > deadline {
                        return Ok(false);
                    }
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
                _ => {
                    if Instant::now() > deadline {
                        return Ok(false);
                    }
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }
}

fn check_output(output: Output) -> Result<(), ClusterError> {
    if output.status.success() {
        Ok(())
    } else {
        Err(ClusterError::Kubectl(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cluster() -> Cluster {
        Cluster {
            backend: Backend::K8s,
            kubectl: "kubectl".into(),
            namespace: "aga".into(),
            template: "/nonexistent/workstation-pod.yaml".into(),
            image: "aga-workstation:test".into(),
            wait_timeout_secs: 1,
        }
    }

    #[test]
    fn workstation_renders_pod_with_git_url_and_branch() {
        let c = cluster();
        let m = c
            .render_workstation_manifest("ws-3", "https://example.com/proj.git", "ws-3", None)
            .unwrap();
        assert!(m.contains("name: ws-3"));
        assert!(m.contains("https://example.com/proj.git"));
        assert!(m.contains("value: \"ws-3\""));
        assert!(m.contains("aga-workstation:test"));
    }

    #[test]
    fn workstation_pod_mounts_named_secret() {
        let c = cluster();
        let m = c
            .render_workstation_manifest("ws-3", "https://x.git", "ws-3", Some("creds"))
            .unwrap();
        // Секрет из кластера монтируется в под по имени, только когда задан.
        assert!(m.contains("secretName: creds"));
        assert!(m.contains("volumeMounts:"));
        assert!(m.contains("mountPath: /etc/secrets/creds"));
        assert!(m.contains("readOnly: true"));
        let plain = c
            .render_workstation_manifest("ws-3", "https://x.git", "ws-3", None)
            .unwrap();
        assert!(!plain.contains("secretName"));
        assert!(!plain.contains("volumeMounts"));
    }

    #[test]
    fn ssh_secret_manifest_carries_base64_key_into_namespace() {
        let c = cluster();
        let m = c
            .render_ssh_secret_manifest("aga-ssh", "private-key-bytes")
            .unwrap();
        assert!(m.contains("kind: Secret"));
        assert!(m.contains("name: aga-ssh"));
        assert!(m.contains("namespace: aga"));
        assert!(m.contains("id_ed25519: cHJpdmF0ZS1rZXktYnl0ZXM="));
    }

    #[test]
    fn each_workstation_gets_its_own_pod() {
        assert_ne!(Cluster::pod_name(1), Cluster::pod_name(2));
        assert_eq!(Cluster::pod_name(1), "ws-1");
    }

    #[test]
    fn workstation_pod_has_no_k8s_api_access() {
        let c = cluster();
        let m = c
            .render_workstation_manifest("ws-1", "https://x.git", "ws-1", None)
            .unwrap();
        assert!(m.contains("automountServiceAccountToken: false"));
        assert!(!m.contains("serviceAccountName"));
    }

    #[test]
    fn docker_run_clones_git_url_like_k8s() {
        assert_eq!(
            Cluster::docker_run_args(
                "ws-3",
                "aga-workstation:dev",
                "https://example.com/proj.git",
                "ws-3"
            ),
            vec![
                "run",
                "-d",
                "--name",
                "ws-3",
                "--privileged",
                "-e",
                "GIT_URL=https://example.com/proj.git",
                "-e",
                "BRANCH=ws-3",
                "aga-workstation:dev",
                "/entrypoint.sh"
            ]
        );
    }

    #[test]
    fn docker_run_mounts_local_project_path() {
        assert_eq!(
            Cluster::docker_run_args(
                "ws-4",
                "aga-workstation:dev",
                "/home/dev/examples/game-xo",
                "ws-4"
            ),
            vec![
                "run",
                "-d",
                "--name",
                "ws-4",
                "--privileged",
                "-v",
                "/home/dev/examples/game-xo:/work/project",
                "aga-workstation:dev",
                "/entrypoint.sh"
            ]
        );
    }
}
