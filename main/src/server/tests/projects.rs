//! Проекты и привязка набора агентов.

use super::*;

#[tokio::test]
async fn participant_sees_all_projects_via_api() {
    let (state, file) = test_state(true).await;
    let headers = auth_headers("alice", &["participant"]);
    let (_, _) = post_json(
        "/projects",
        &headers,
        state.clone(),
        serde_json::json!({"git_url": "https://example.com/a.git"}),
    )
    .await;
    let (status, _) = get("/projects", &headers, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    cleanup(&file).await;
}

#[tokio::test]
async fn participant_creates_project_visible_to_others() {
    let (state, file) = test_state(true).await;
    let alice = auth_headers("alice", &["participant"]);
    let (status, _) = post_json(
        "/projects",
        &alice,
        state.clone(),
        serde_json::json!({"git_url": "https://example.com/a.git"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // Проект виден другому участнику.
    let bob = auth_headers("bob", &["participant"]);
    let (status, body) = get("/projects", &bob, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("example.com/a.git"));
    cleanup(&file).await;
}

/// git_url уходит в git clone на станции: мусор и попытки инъекции — 400.
#[tokio::test]
async fn project_with_hostile_git_url_rejected() {
    let (state, file) = test_state(true).await;
    let alice = auth_headers("alice", &["participant"]);
    for bad in [
        "x'; touch /tmp/pwned; echo '",
        "--upload-pack=touch /tmp/pwned",
        "not a url",
    ] {
        let (status, _) = post_json(
            "/projects",
            &alice,
            state.clone(),
            serde_json::json!({ "git_url": bad }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    }
    let (status, body) = post_json(
        "/projects",
        &alice,
        state.clone(),
        serde_json::json!({ "git_url": " git@github.com:a/b.git " }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("\"git_url\":\"git@github.com:a/b.git\""),
        "{body}"
    );
    cleanup(&file).await;
}

#[tokio::test]
async fn attached_agent_set_appears_on_project() {
    let (state, file) = test_state(true).await;
    let headers = auth_headers("alice", &["participant"]);
    let project_id = state
        .trace_store
        .upsert_project("https://example.com/a.git")
        .await
        .unwrap();
    let set_id = state
        .trace_store
        .create_agent_set(
            "ops",
            &[crate::trace::AgentSpec {
                name: "dev".to_string(),
                description: "Правила разработчика".to_string(),
                tools: vec!["git".to_string()],
                max_iterations: 3,
                llm_id: None,
                parent: None,
                skills: vec![],
                listen_user_id: None,
                mcp: vec![],
            }],
        )
        .await
        .unwrap();
    let (status, _) = post_json(
        &format!("/projects/{project_id}/agent-set"),
        &headers,
        state.clone(),
        serde_json::json!({ "agent_set_id": set_id }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = get("/projects", &headers, state.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("dev"));
    cleanup(&file).await;
}

#[tokio::test]
async fn replacing_agent_set_changes_project_agents_via_api() {
    let (state, file) = test_state(true).await;
    let headers = auth_headers("alice", &["participant"]);
    let project_id = state
        .trace_store
        .upsert_project("https://example.com/r.git")
        .await
        .unwrap();
    let set_a = state
        .trace_store
        .create_agent_set(
            "set-a",
            &[crate::trace::AgentSpec {
                name: "dev-a".to_string(),
                description: "a".to_string(),
                tools: vec![],
                max_iterations: 1,
                llm_id: None,
                parent: None,
                skills: vec![],
                listen_user_id: None,
                mcp: vec![],
            }],
        )
        .await
        .unwrap();
    let set_b = state
        .trace_store
        .create_agent_set(
            "set-b",
            &[crate::trace::AgentSpec {
                name: "dev-b".to_string(),
                description: "b".to_string(),
                tools: vec![],
                max_iterations: 1,
                llm_id: None,
                parent: None,
                skills: vec![],
                listen_user_id: None,
                mcp: vec![],
            }],
        )
        .await
        .unwrap();
    let (s, _) = post_json(
        &format!("/projects/{project_id}/agent-set"),
        &headers,
        state.clone(),
        serde_json::json!({ "agent_set_id": set_a }),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = post_json(
        &format!("/projects/{project_id}/agent-set"),
        &headers,
        state.clone(),
        serde_json::json!({ "agent_set_id": set_b }),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, body) = get(
        &format!("/projects/{project_id}/agent-set"),
        &headers,
        state.clone(),
    )
    .await;
    assert!(body.contains("dev-b"));
    assert!(!body.contains("dev-a"));
    cleanup(&file).await;
}
