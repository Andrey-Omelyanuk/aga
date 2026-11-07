use super::*;
use crate::config::LlmConfig;
use sqlx::Row;
use std::collections::HashMap;

impl TraceStore {
    // === Методы для управления AgentSet-ами ===

    /// Создать набор агентов. Агенты кладутся сразу: parent — имя родителя в
    /// дереве (агент папки, наследником которого становится этот агент);
    /// skills — имена записей каталога (привязываются, если есть).
    pub async fn create_agent_set(
        &self,
        name: &str,
        specs: &[AgentSpec],
    ) -> Result<i64, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let result = sqlx::query("INSERT INTO agent_sets (name) VALUES (?)")
            .bind(name)
            .execute(&mut *tx)
            .await?;
        let set_id = result.last_insert_rowid();
        Self::insert_agent_specs(&mut tx, set_id, specs).await?;
        tx.commit().await?;
        Ok(set_id)
    }

    /// Полностью заменить состав набора: имя и агенты с их инструментами,
    /// скиллами и деревом. Возвращает false, если набора нет.
    pub async fn update_agent_set(
        &self,
        set_id: i64,
        name: &str,
        specs: &[AgentSpec],
    ) -> Result<bool, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let result = sqlx::query("UPDATE agent_sets SET name = ? WHERE id = ?")
            .bind(name)
            .bind(set_id)
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() == 0 {
            tx.rollback().await?;
            return Ok(false);
        }
        // Состав заменяется целиком: старые агенты (и их способности каскадно)
        // удаляются, новые кладутся заново.
        sqlx::query("DELETE FROM agents WHERE set_id = ?")
            .bind(set_id)
            .execute(&mut *tx)
            .await?;
        Self::insert_agent_specs(&mut tx, set_id, specs).await?;
        tx.commit().await?;
        Ok(true)
    }

    async fn insert_agent_specs(
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        set_id: i64,
        specs: &[AgentSpec],
    ) -> Result<(), sqlx::Error> {
        let mut ids: HashMap<String, i64> = HashMap::new();
        for spec in specs {
            let parent_id: Option<i64> = spec.parent.as_deref().and_then(|p| ids.get(p).copied());
            let tools = serde_json::to_string(&spec.tools).unwrap_or_else(|_| "[]".into());
            let result = sqlx::query(
                "INSERT INTO agents (set_id, name, description, tools, max_iterations, llm_id, parent_id, listen_user_id)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(set_id)
            .bind(&spec.name)
            .bind(&spec.description)
            .bind(&tools)
            .bind(spec.max_iterations as i64)
            .bind(spec.llm_id)
            .bind(parent_id)
            .bind(spec.listen_user_id)
            .execute(&mut **tx)
            .await?;
            let agent_id = result.last_insert_rowid();
            ids.insert(spec.name.clone(), agent_id);

            for cap in &spec.skills {
                Self::link_capability(&mut *tx, agent_id, CapabilityKind::Skill, cap).await?;
            }
            for name in &spec.mcp {
                sqlx::query(
                    "INSERT OR IGNORE INTO agent_mcp_servers (agent_id, server_id)
                     SELECT ?, id FROM mcp_servers WHERE name = ?",
                )
                .bind(agent_id)
                .bind(name)
                .execute(&mut **tx)
                .await?;
            }
        }
        Ok(())
    }

    /// Привязать способность каталога к агенту. Имени нет в каталоге — не
    /// привязываем (набор создаётся даже с отложенными ссылками).
    async fn link_capability(
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        agent_id: i64,
        kind: CapabilityKind,
        cap: &AgentCapability,
    ) -> Result<(), sqlx::Error> {
        let row = sqlx::query("SELECT id FROM capabilities WHERE kind = ? AND name = ?")
            .bind(kind.as_str())
            .bind(&cap.name)
            .fetch_optional(&mut **tx)
            .await?;
        let Some(row) = row else {
            return Ok(());
        };
        let capability_id: i64 = row.get("id");
        sqlx::query("INSERT INTO agent_capabilities (agent_id, capability_id) VALUES (?, ?)")
            .bind(agent_id)
            .bind(capability_id)
            .execute(&mut **tx)
            .await?;
        Ok(())
    }

    async fn load_set(&self, set_id: i64) -> Result<Option<AgentSet>, sqlx::Error> {
        let name_row = sqlx::query("SELECT name FROM agent_sets WHERE id = ?")
            .bind(set_id)
            .fetch_optional(&self.pool)
            .await?;
        let Some(name_row) = name_row else {
            return Ok(None);
        };
        let name: String = name_row.get("name");
        let rows = sqlx::query(
            "SELECT id, name, description, tools, max_iterations, llm_id, parent_id, listen_user_id
             FROM agents WHERE set_id = ? ORDER BY id",
        )
        .bind(set_id)
        .fetch_all(&self.pool)
        .await?;
        let mut agents = Vec::new();
        for r in rows {
            let tools_json: String = r.get("tools");
            let tools = serde_json::from_str(&tools_json).unwrap_or_else(|_| Vec::new());
            let agent_id: i64 = r.get("id");
            let caps = sqlx::query(
                "SELECT c.kind, c.name
                 FROM agent_capabilities ac JOIN capabilities c ON c.id = ac.capability_id
                 WHERE ac.agent_id = ? ORDER BY c.kind, c.name",
            )
            .bind(agent_id)
            .fetch_all(&self.pool)
            .await?;
            let mut skills = Vec::new();
            for cap in caps {
                let kind: String = cap.get("kind");
                if kind == "skill" {
                    skills.push(AgentCapability {
                        name: cap.get("name"),
                    });
                }
            }
            let mcp = sqlx::query(
                "SELECT m.name FROM agent_mcp_servers am JOIN mcp_servers m ON m.id = am.server_id
                 WHERE am.agent_id = ? ORDER BY m.name",
            )
            .bind(agent_id)
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(|r| r.get::<String, _>("name"))
            .collect();
            agents.push(AgentDef {
                id: agent_id,
                name: r.get("name"),
                description: r.get("description"),
                tools,
                max_iterations: r.get("max_iterations"),
                llm_id: r.get("llm_id"),
                parent_id: r.get("parent_id"),
                skills,
                territory: Default::default(),
                listen_user_id: r.get("listen_user_id"),
                mcp,
            });
        }
        let mut set = AgentSet {
            id: set_id,
            name,
            agents,
        };
        // Территория агента — папка его узла в дереве минус папки наследников.
        // Снимок списка — чтобы не держать set заимствованным на время правки.
        let snapshot = set.agents.clone();
        for agent in &mut set.agents {
            agent.territory = crate::scope::territory_for_list(&snapshot, agent);
        }
        Ok(Some(set))
    }

    pub async fn list_agent_sets(&self) -> Result<Vec<AgentSet>, sqlx::Error> {
        let rows = sqlx::query("SELECT id FROM agent_sets ORDER BY id")
            .fetch_all(&self.pool)
            .await?;
        let mut sets = Vec::new();
        for r in rows {
            let id: i64 = r.get("id");
            if let Some(set) = self.load_set(id).await? {
                sets.push(set);
            }
        }
        Ok(sets)
    }

    pub async fn get_agent_set(&self, set_id: i64) -> Result<Option<AgentSet>, sqlx::Error> {
        self.load_set(set_id).await
    }

    pub async fn delete_agent_set(&self, set_id: i64) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM agent_sets WHERE id = ?")
            .bind(set_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// LLM-конфиг агента: url, ключ и модель берутся из выбранного подключения,
    /// а без него — из дефолтной LLM (выбранной на странице «LLM»). Своей
    /// модели и температуры у агента нет — модель живёт в подключении,
    /// температура 0.7. Ни подключения, ни дефолтной LLM нет — конфиг без url:
    /// запуск агента не пройдёт (LlmClient ответит, что LLM не настроена).
    pub async fn llm_config_for(&self, agent: &AgentDef) -> Result<LlmConfig, sqlx::Error> {
        let mut llm = LlmConfig {
            model: None,
            temperature: 0.7,
            api_url: None,
            api_key: None,
            native_tools: true,
            max_tokens: None,
        };
        // Своё подключение важнее дефолта; ссылка на удалённое — как отсутствие.
        let conn = match agent.llm_id {
            Some(id) => self.get_llm_connection(id).await?,
            None => None,
        };
        let conn = conn.or(self.default_llm_connection().await?);
        if let Some(conn) = conn {
            llm.model = Some(conn.model_name);
            llm.api_url = Some(conn.api_url);
            llm.api_key = conn.api_key;
            llm.native_tools = conn.native_tools;
            llm.max_tokens = conn.max_tokens;
            if let Some(t) = conn.temperature {
                llm.temperature = t as f32;
            }
        }
        Ok(llm)
    }

    /// Прикрепить набор к проекту. Один набор замещает прежний (project_id —
    /// PK): повторная привязка меняет набор проекта.
    pub async fn attach_agent_set(&self, project_id: i64, set_id: i64) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO project_agent_set (project_id, agent_set_id) VALUES (?, ?)
             ON CONFLICT(project_id) DO UPDATE SET agent_set_id = excluded.agent_set_id",
        )
        .bind(project_id)
        .bind(set_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_project_agent_set(
        &self,
        project_id: i64,
    ) -> Result<Option<AgentSet>, sqlx::Error> {
        let row = sqlx::query("SELECT agent_set_id FROM project_agent_set WHERE project_id = ?")
            .bind(project_id)
            .fetch_optional(&self.pool)
            .await?;
        match row {
            Some(r) => self.load_set(r.get("agent_set_id")).await,
            None => Ok(None),
        }
    }

    /// Агент из набора проекта, привязанный к пользователю (`listen_user_id`).
    /// Использует подписчик (`runtime.rs`): по событию сообщения находит, есть ли
    /// в наборе проекта чата агент, слушающий автора, — и запускает его. Нет
    /// привязки — None, агент не реагирует.
    pub fn agent_listening_to(&self, set: &AgentSet, user_id: i64) -> Option<AgentDef> {
        set.agents
            .iter()
            .find(|a| a.listen_user_id == Some(user_id))
            .cloned()
    }

    /// Различные id пользователей, к которым привязаны агенты, по порядку.
    /// Это каналы (`user:<id>`), на которые подписывается агент-рантайм.
    pub async fn listen_user_ids(&self) -> Result<Vec<i64>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT DISTINCT listen_user_id FROM agents
             WHERE listen_user_id IS NOT NULL
             ORDER BY listen_user_id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(|r| r.get("listen_user_id")).collect())
    }

    /// Промпт агента: его правила плюс данные ему скиллы, раскрытые в их
    /// единственном текущем содержимом.
    pub async fn agent_prompt(&self, agent: &AgentDef) -> Result<String, sqlx::Error> {
        let mut parts = vec![agent.description.clone()];
        for sk in &agent.skills {
            if let Some(content) = self
                .resolve_capability(CapabilityKind::Skill, &sk.name)
                .await?
            {
                parts.push(format!("Скилл «{}»: {}", sk.name, content));
            }
        }
        Ok(parts.join("\n\n"))
    }
}
