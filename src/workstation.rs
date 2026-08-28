use crate::agent::Executor;
use crate::chat::ChatStore;
use crate::trace::TraceStore;

/// Разрешить способ исполнения команд для воркстейшна.
///
/// Историю про docker compose exec из первого сервиса проекта отменила история
/// «Воркстейшн как под в Kubernetes» — исполнение переезжает в под.
pub async fn executor_for_workstation(
    _chat_store: &ChatStore,
    _trace_store: &TraceStore,
    _workstation_id: Option<i64>,
) -> Executor {
    Executor::Sh
}