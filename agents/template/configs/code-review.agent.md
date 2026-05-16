# Code Review Agent Configuration

## Role Definition
role: code-review
description: Reviews code for quality, security issues, best practices, and potential bugs

## Capabilities
- code_review
- static_analysis
- security_scanning
- best_practices_checking

## Tools
- file_read
- search_replace
- code_execution
- vulnerability_detection

## Model Configuration
model: codellama-13b
model_path: /models/codellama-13b
temperature: 0.3
max_tokens: 2048

## Communication
nats_subject: agent.code-review.task
status_topic: agent.code-review.status
result_topic: agent.code-review.result

## Database Schema
schema:
  - table_name: code_reviews
    columns:
      - name: id
        type: uuid
        primary_key: true
      - name: task_id
        type: uuid
        foreign_key: tasks.id
      - name: review_score
        type: integer
      - name: issues_found
        type: jsonb
      - name: recommendations
        type: text
      - name: created_at
        type: timestamp
