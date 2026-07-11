use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Environment {
    pub id: Option<i64>,
    pub name: String,
    pub keystone_url: String,
    pub user_name: String,
    pub user_domain: String,
    pub project_name: String,
    pub project_domain: String,
    pub mq_url: String,
    pub mq_exchanges: String, // 쉼표 구분: "nova,neutron,cinder"
    pub endpoints: HashMap<String, String>, // {"nova": "http://nova:8774/v2.1", ...}
    // RabbitMQ 클러스터: 여러 host:port(쉼표 구분) + 계정. 접속 시 순서대로 시도(페일오버).
    #[serde(default)]
    pub mq_hosts: String, // "10.255.40.2:5672,10.255.40.3:5672,10.255.40.4:5672"
    #[serde(default)]
    pub mq_user: String,
    #[serde(default)]
    pub mq_password: String,
    #[serde(default)]
    pub mq_vhost: String, // 기본 "/"
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScenarioRecord {
    pub id: Option<i64>,
    pub name: String,
    pub description: String,
    pub steps_json: String, // models::Scenario의 steps 배열 JSON
}

#[derive(Debug, Clone, Serialize)]
pub struct RunRecord {
    pub id: i64,
    pub scenario_id: i64,
    pub scenario_name: String,
    pub env_id: i64,
    pub status: String, // running | passed | failed | cancelled | interrupted
    pub started_at: String,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StepResultRecord {
    pub run_id: i64,
    pub step_index: i64,
    pub name: String,
    pub status: String,
    pub detail: String,
    pub duration_ms: i64,
}

/// DB에 저장된 UI 동작 플로우 (사이트 URL별, 이름 유니크).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UiFlowRecord {
    pub id: Option<i64>,
    pub name: String,
    pub site_url: String,
    #[serde(default)]
    pub grp: String, // 트리 그룹(폴더). 빈 문자열이면 '기본'.
    pub actions_json: String, // UiAction[] JSON
}

#[derive(Debug, Clone, Serialize)]
pub struct UiFlowSite {
    pub site_url: String,
    pub count: i64,
}

/// UI 스위트/레코더에서 실행한 흐름 1회의 기록.
#[derive(Debug, Clone, Serialize)]
pub struct UiRunRecord {
    pub id: i64,
    pub flow_id: Option<i64>,
    pub flow_name: String,
    pub site_url: String,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
}

/// UI 실행 1회의 스텝별 결과.
#[derive(Debug, Clone, Serialize)]
pub struct UiRunStepRecord {
    pub run_id: i64,
    pub step_index: i64,
    pub kind: String,
    pub name: String,
    pub status: String,
    pub detail: String,
}

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &std::path::Path) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self, String> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS environments (
               id INTEGER PRIMARY KEY,
               name TEXT NOT NULL,
               keystone_url TEXT NOT NULL,
               user_name TEXT NOT NULL,
               user_domain TEXT NOT NULL,
               project_name TEXT NOT NULL,
               project_domain TEXT NOT NULL,
               mq_url TEXT NOT NULL,
               mq_exchanges TEXT NOT NULL,
               endpoints TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS scenarios (
               id INTEGER PRIMARY KEY,
               name TEXT NOT NULL,
               description TEXT NOT NULL DEFAULT '',
               steps_json TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS runs (
               id INTEGER PRIMARY KEY,
               scenario_id INTEGER NOT NULL,
               env_id INTEGER NOT NULL,
               status TEXT NOT NULL,
               started_at TEXT NOT NULL,
               finished_at TEXT
             );
             CREATE TABLE IF NOT EXISTS step_results (
               id INTEGER PRIMARY KEY,
               run_id INTEGER NOT NULL,
               step_index INTEGER NOT NULL,
               name TEXT NOT NULL,
               status TEXT NOT NULL,
               detail TEXT NOT NULL,
               duration_ms INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS ui_flows (
               id INTEGER PRIMARY KEY,
               name TEXT NOT NULL,
               site_url TEXT NOT NULL,
               actions_json TEXT NOT NULL,
               updated_at TEXT NOT NULL,
               UNIQUE(site_url, name)
             );
             CREATE TABLE IF NOT EXISTS ui_runs (
               id INTEGER PRIMARY KEY,
               flow_id INTEGER,
               flow_name TEXT NOT NULL,
               site_url TEXT NOT NULL,
               status TEXT NOT NULL,
               started_at TEXT NOT NULL,
               finished_at TEXT
             );
             CREATE TABLE IF NOT EXISTS ui_run_steps (
               id INTEGER PRIMARY KEY,
               run_id INTEGER NOT NULL,
               step_index INTEGER NOT NULL,
               kind TEXT NOT NULL,
               name TEXT NOT NULL,
               status TEXT NOT NULL,
               detail TEXT NOT NULL
             );",
        )
        .map_err(|e| e.to_string())?;
        // 마이그레이션: 기존 DB의 ui_flows 에 그룹(grp) 컬럼 추가 (이미 있으면 무시).
        if !Self::has_column(&conn, "ui_flows", "grp") {
            conn.execute("ALTER TABLE ui_flows ADD COLUMN grp TEXT NOT NULL DEFAULT ''", [])
                .map_err(|e| e.to_string())?;
        }
        // 마이그레이션: RabbitMQ 클러스터 호스트/계정 컬럼 추가.
        for (col, default) in [
            ("mq_hosts", "''"),
            ("mq_user", "''"),
            ("mq_password", "''"),
            ("mq_vhost", "'/'"),
        ] {
            if !Self::has_column(&conn, "environments", col) {
                conn.execute(
                    &format!("ALTER TABLE environments ADD COLUMN {col} TEXT NOT NULL DEFAULT {default}"),
                    [],
                )
                .map_err(|e| e.to_string())?;
            }
        }
        Ok(Self { conn })
    }

    fn has_column(conn: &Connection, table: &str, column: &str) -> bool {
        conn.prepare(&format!("PRAGMA table_info({table})"))
            .and_then(|mut stmt| {
                let cols: Vec<String> = stmt
                    .query_map([], |r| r.get::<_, String>(1))?
                    .filter_map(Result::ok)
                    .collect();
                Ok(cols.iter().any(|c| c == column))
            })
            .unwrap_or(false)
    }

    // --- environments ---

    pub fn save_environment(&self, env: &Environment) -> Result<i64, String> {
        let endpoints = serde_json::to_string(&env.endpoints).map_err(|e| e.to_string())?;
        match env.id {
            Some(id) => {
                self.conn
                    .execute(
                        "UPDATE environments SET name=?1, keystone_url=?2, user_name=?3, user_domain=?4,
                         project_name=?5, project_domain=?6, mq_url=?7, mq_exchanges=?8, endpoints=?9,
                         mq_hosts=?11, mq_user=?12, mq_password=?13, mq_vhost=?14 WHERE id=?10",
                        params![env.name, env.keystone_url, env.user_name, env.user_domain,
                                env.project_name, env.project_domain, env.mq_url, env.mq_exchanges, endpoints, id,
                                env.mq_hosts, env.mq_user, env.mq_password, env.mq_vhost],
                    )
                    .map_err(|e| e.to_string())?;
                Ok(id)
            }
            None => {
                self.conn
                    .execute(
                        "INSERT INTO environments (name, keystone_url, user_name, user_domain,
                         project_name, project_domain, mq_url, mq_exchanges, endpoints,
                         mq_hosts, mq_user, mq_password, mq_vhost)
                         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
                        params![env.name, env.keystone_url, env.user_name, env.user_domain,
                                env.project_name, env.project_domain, env.mq_url, env.mq_exchanges, endpoints,
                                env.mq_hosts, env.mq_user, env.mq_password, env.mq_vhost],
                    )
                    .map_err(|e| e.to_string())?;
                Ok(self.conn.last_insert_rowid())
            }
        }
    }

    pub fn list_environments(&self) -> Result<Vec<Environment>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, keystone_url, user_name, user_domain, project_name, project_domain, mq_url, mq_exchanges, endpoints, mq_hosts, mq_user, mq_password, mq_vhost FROM environments ORDER BY id")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                let endpoints_json: String = r.get(9)?;
                let endpoints = serde_json::from_str(&endpoints_json).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        9,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                Ok(Environment {
                    id: Some(r.get(0)?),
                    name: r.get(1)?,
                    keystone_url: r.get(2)?,
                    user_name: r.get(3)?,
                    user_domain: r.get(4)?,
                    project_name: r.get(5)?,
                    project_domain: r.get(6)?,
                    mq_url: r.get(7)?,
                    mq_exchanges: r.get(8)?,
                    endpoints,
                    mq_hosts: r.get(10)?,
                    mq_user: r.get(11)?,
                    mq_password: r.get(12)?,
                    mq_vhost: r.get(13)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
    }

    pub fn get_environment(&self, id: i64) -> Result<Environment, String> {
        self.list_environments()?
            .into_iter()
            .find(|e| e.id == Some(id))
            .ok_or_else(|| format!("환경 {id} 없음"))
    }

    pub fn delete_environment(&self, id: i64) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM environments WHERE id=?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    // --- scenarios ---

    pub fn save_scenario(&self, s: &ScenarioRecord) -> Result<i64, String> {
        // steps_json이 유효한 스텝 배열인지 저장 전에 검증
        serde_json::from_str::<Vec<crate::models::StepDef>>(&s.steps_json)
            .map_err(|e| format!("스텝 JSON이 유효하지 않음: {e}"))?;
        match s.id {
            Some(id) => {
                self.conn
                    .execute(
                        "UPDATE scenarios SET name=?1, description=?2, steps_json=?3 WHERE id=?4",
                        params![s.name, s.description, s.steps_json, id],
                    )
                    .map_err(|e| e.to_string())?;
                Ok(id)
            }
            None => {
                self.conn
                    .execute(
                        "INSERT INTO scenarios (name, description, steps_json) VALUES (?1,?2,?3)",
                        params![s.name, s.description, s.steps_json],
                    )
                    .map_err(|e| e.to_string())?;
                Ok(self.conn.last_insert_rowid())
            }
        }
    }

    pub fn list_scenarios(&self) -> Result<Vec<ScenarioRecord>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, description, steps_json FROM scenarios ORDER BY id")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok(ScenarioRecord {
                    id: Some(r.get(0)?),
                    name: r.get(1)?,
                    description: r.get(2)?,
                    steps_json: r.get(3)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
    }

    pub fn get_scenario(&self, id: i64) -> Result<ScenarioRecord, String> {
        self.list_scenarios()?
            .into_iter()
            .find(|s| s.id == Some(id))
            .ok_or_else(|| format!("시나리오 {id} 없음"))
    }

    pub fn delete_scenario(&self, id: i64) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM scenarios WHERE id=?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    // --- ui_flows (UI 레코더 플로우) ---

    pub fn save_ui_flow(
        &self,
        name: &str,
        site_url: &str,
        grp: &str,
        actions_json: &str,
        updated_at: &str,
    ) -> Result<i64, String> {
        self.conn
            .execute(
                "INSERT INTO ui_flows (name, site_url, grp, actions_json, updated_at) VALUES (?1,?2,?3,?4,?5)
                 ON CONFLICT(site_url, name) DO UPDATE SET grp=?3, actions_json=?4, updated_at=?5",
                params![name, site_url, grp, actions_json, updated_at],
            )
            .map_err(|e| e.to_string())?;
        self.conn
            .query_row(
                "SELECT id FROM ui_flows WHERE site_url=?1 AND name=?2",
                params![site_url, name],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())
    }

    pub fn list_ui_flow_sites(&self) -> Result<Vec<UiFlowSite>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT site_url, COUNT(*) FROM ui_flows GROUP BY site_url ORDER BY site_url")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| Ok(UiFlowSite { site_url: r.get(0)?, count: r.get(1)? }))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    fn map_ui_flow(r: &rusqlite::Row) -> rusqlite::Result<UiFlowRecord> {
        Ok(UiFlowRecord {
            id: Some(r.get(0)?),
            name: r.get(1)?,
            site_url: r.get(2)?,
            grp: r.get(3)?,
            actions_json: r.get(4)?,
        })
    }

    pub fn list_ui_flows(&self, site_url: &str) -> Result<Vec<UiFlowRecord>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, site_url, grp, actions_json FROM ui_flows WHERE site_url=?1 ORDER BY grp, name")
            .map_err(|e| e.to_string())?;
        let rows = stmt.query_map(params![site_url], Self::map_ui_flow).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn all_ui_flows(&self) -> Result<Vec<UiFlowRecord>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, site_url, grp, actions_json FROM ui_flows ORDER BY site_url, grp, name")
            .map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], Self::map_ui_flow).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn delete_ui_flow(&self, id: i64) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM ui_flows WHERE id=?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 시나리오 이름 변경(같은 사이트에 이름 중복이면 UNIQUE 제약으로 실패).
    pub fn rename_ui_flow(&self, id: i64, new_name: &str) -> Result<(), String> {
        self.conn
            .execute("UPDATE ui_flows SET name=?1 WHERE id=?2", params![new_name, id])
            .map_err(|e| format!("이름 변경 실패(중복 가능): {e}"))?;
        Ok(())
    }

    /// 그룹명 일괄 변경(해당 사이트에서 old_grp 인 시나리오 전부). 변경된 개수 반환.
    pub fn rename_ui_group(&self, site_url: &str, old_grp: &str, new_grp: &str) -> Result<usize, String> {
        self.conn
            .execute(
                "UPDATE ui_flows SET grp=?1 WHERE site_url=?2 AND grp=?3",
                params![new_grp, site_url, old_grp],
            )
            .map_err(|e| e.to_string())
    }

    // --- runs / step_results ---

    pub fn create_run(&self, scenario_id: i64, env_id: i64, started_at: &str) -> Result<i64, String> {
        self.conn
            .execute(
                "INSERT INTO runs (scenario_id, env_id, status, started_at) VALUES (?1,?2,'running',?3)",
                params![scenario_id, env_id, started_at],
            )
            .map_err(|e| e.to_string())?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn finish_run(&self, run_id: i64, status: &str, finished_at: &str) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE runs SET status=?1, finished_at=?2 WHERE id=?3",
                params![status, finished_at, run_id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 앱 시작 시 호출: 이전 세션에서 'running'으로 남은 좀비 run을 interrupted로 정리한다.
    pub fn mark_interrupted_runs(&self, finished_at: &str) -> Result<usize, String> {
        self.conn
            .execute(
                "UPDATE runs SET status='interrupted', finished_at=?1 WHERE status='running'",
                params![finished_at],
            )
            .map_err(|e| e.to_string())
    }

    pub fn list_runs(&self) -> Result<Vec<RunRecord>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT r.id, r.scenario_id, COALESCE(s.name, '(삭제됨)'), r.env_id, r.status, r.started_at, r.finished_at
                 FROM runs r LEFT JOIN scenarios s ON s.id = r.scenario_id ORDER BY r.id DESC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok(RunRecord {
                    id: r.get(0)?,
                    scenario_id: r.get(1)?,
                    scenario_name: r.get(2)?,
                    env_id: r.get(3)?,
                    status: r.get(4)?,
                    started_at: r.get(5)?,
                    finished_at: r.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
    }

    pub fn save_step_result(&self, r: &StepResultRecord) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO step_results (run_id, step_index, name, status, detail, duration_ms)
                 VALUES (?1,?2,?3,?4,?5,?6)",
                params![r.run_id, r.step_index, r.name, r.status, r.detail, r.duration_ms],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn list_step_results(&self, run_id: i64) -> Result<Vec<StepResultRecord>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT run_id, step_index, name, status, detail, duration_ms FROM step_results WHERE run_id=?1 ORDER BY step_index")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([run_id], |r| {
                Ok(StepResultRecord {
                    run_id: r.get(0)?,
                    step_index: r.get(1)?,
                    name: r.get(2)?,
                    status: r.get(3)?,
                    detail: r.get(4)?,
                    duration_ms: r.get(5)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
    }

    // --- ui_runs / ui_run_steps (UI 스위트 실행 히스토리) ---

    pub fn create_ui_run(
        &self,
        flow_id: Option<i64>,
        flow_name: &str,
        site_url: &str,
        started_at: &str,
    ) -> Result<i64, String> {
        self.conn
            .execute(
                "INSERT INTO ui_runs (flow_id, flow_name, site_url, status, started_at) VALUES (?1,?2,?3,'running',?4)",
                params![flow_id, flow_name, site_url, started_at],
            )
            .map_err(|e| e.to_string())?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn save_ui_run_step(&self, r: &UiRunStepRecord) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO ui_run_steps (run_id, step_index, kind, name, status, detail) VALUES (?1,?2,?3,?4,?5,?6)",
                params![r.run_id, r.step_index, r.kind, r.name, r.status, r.detail],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn finish_ui_run(&self, run_id: i64, status: &str, finished_at: &str) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE ui_runs SET status=?1, finished_at=?2 WHERE id=?3",
                params![status, finished_at, run_id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 앱 시작 시: 이전 세션에서 'running'으로 남은 UI 실행을 interrupted로 정리.
    pub fn mark_interrupted_ui_runs(&self, finished_at: &str) -> Result<usize, String> {
        self.conn
            .execute(
                "UPDATE ui_runs SET status='interrupted', finished_at=?1 WHERE status='running'",
                params![finished_at],
            )
            .map_err(|e| e.to_string())
    }

    pub fn list_ui_runs(&self) -> Result<Vec<UiRunRecord>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, flow_id, flow_name, site_url, status, started_at, finished_at FROM ui_runs ORDER BY id DESC")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok(UiRunRecord {
                    id: r.get(0)?,
                    flow_id: r.get(1)?,
                    flow_name: r.get(2)?,
                    site_url: r.get(3)?,
                    status: r.get(4)?,
                    started_at: r.get(5)?,
                    finished_at: r.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
    }

    pub fn list_ui_run_steps(&self, run_id: i64) -> Result<Vec<UiRunStepRecord>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT run_id, step_index, kind, name, status, detail FROM ui_run_steps WHERE run_id=?1 ORDER BY step_index")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([run_id], |r| {
                Ok(UiRunStepRecord {
                    run_id: r.get(0)?,
                    step_index: r.get(1)?,
                    kind: r.get(2)?,
                    name: r.get(3)?,
                    status: r.get(4)?,
                    detail: r.get(5)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
    }
}

// --- OS 키체인 (환경 비밀번호) ---
// 주의: 단위 테스트 없음 — OS 키체인을 건드리므로 Task 12 이후 수동 검증.

const KEYRING_SERVICE: &str = "contrabass-test-tool";

pub fn save_password(env_id: i64, password: &str) -> Result<(), String> {
    keyring::Entry::new(KEYRING_SERVICE, &format!("env-{env_id}"))
        .and_then(|e| e.set_password(password))
        .map_err(|e| format!("키체인 저장 실패: {e}"))
}

pub fn get_password(env_id: i64) -> Result<String, String> {
    keyring::Entry::new(KEYRING_SERVICE, &format!("env-{env_id}"))
        .and_then(|e| e.get_password())
        .map_err(|e| format!("키체인 조회 실패 (환경 비밀번호를 다시 저장하세요): {e}"))
}

pub fn delete_password(env_id: i64) {
    if let Ok(e) = keyring::Entry::new(KEYRING_SERVICE, &format!("env-{env_id}")) {
        if let Err(err) = e.delete_credential() {
            eprintln!("[store] 키체인 항목 삭제 실패 (env-{env_id}): {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> Environment {
        Environment {
            id: None,
            name: "dev".into(),
            keystone_url: "http://ks:5000".into(),
            user_name: "admin".into(),
            user_domain: "Default".into(),
            project_name: "admin".into(),
            project_domain: "Default".into(),
            mq_url: "amqp://guest:guest@mq:5672/%2f".into(),
            mq_exchanges: "nova,neutron,cinder".into(),
            endpoints: std::collections::HashMap::from([("nova".to_string(), "http://nova:8774/v2.1".to_string())]),
            mq_hosts: "mq:5672".into(),
            mq_user: "guest".into(),
            mq_password: "guest".into(),
            mq_vhost: "/".into(),
        }
    }

    #[test]
    fn environment_crud() {
        let store = Store::open_in_memory().unwrap();
        let id = store.save_environment(&env()).unwrap();
        let mut loaded = store.get_environment(id).unwrap();
        assert_eq!(loaded.name, "dev");
        assert_eq!(loaded.endpoints["nova"], "http://nova:8774/v2.1");

        loaded.name = "dev2".into();
        store.save_environment(&loaded).unwrap();
        assert_eq!(store.get_environment(id).unwrap().name, "dev2");

        store.delete_environment(id).unwrap();
        assert!(store.list_environments().unwrap().is_empty());
    }

    #[test]
    fn scenario_crud_validates_steps_json() {
        let store = Store::open_in_memory().unwrap();
        let good = ScenarioRecord {
            id: None,
            name: "s1".into(),
            description: String::new(),
            steps_json: r#"[{"name":"대기","type":"sleep","seconds":1}]"#.into(),
        };
        let id = store.save_scenario(&good).unwrap();
        assert_eq!(store.get_scenario(id).unwrap().name, "s1");

        let bad = ScenarioRecord { steps_json: "not json".into(), ..good };
        assert!(store.save_scenario(&bad).is_err());
    }

    #[test]
    fn run_lifecycle_and_step_results() {
        let store = Store::open_in_memory().unwrap();
        let sid = store
            .save_scenario(&ScenarioRecord {
                id: None,
                name: "s".into(),
                description: String::new(),
                steps_json: "[]".into(),
            })
            .unwrap();
        let run_id = store.create_run(sid, 1, "2026-07-06T00:00:00Z").unwrap();
        store
            .save_step_result(&StepResultRecord {
                run_id,
                step_index: 0,
                name: "스텝".into(),
                status: "passed".into(),
                detail: "HTTP 202".into(),
                duration_ms: 42,
            })
            .unwrap();
        store.finish_run(run_id, "passed", "2026-07-06T00:01:00Z").unwrap();

        let runs = store.list_runs().unwrap();
        assert_eq!(runs[0].status, "passed");
        assert_eq!(runs[0].scenario_name, "s");
        let results = store.list_step_results(run_id).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].duration_ms, 42);

        store.delete_scenario(sid).unwrap();
        assert_eq!(store.list_runs().unwrap()[0].scenario_name, "(삭제됨)");
    }

    #[test]
    fn marks_zombie_running_runs_as_interrupted() {
        let store = Store::open_in_memory().unwrap();
        let sid = store
            .save_scenario(&ScenarioRecord {
                id: None,
                name: "s".into(),
                description: String::new(),
                steps_json: "[]".into(),
            })
            .unwrap();
        store.create_run(sid, 1, "2026-07-06T00:00:00Z").unwrap();

        let n = store.mark_interrupted_runs("2026-07-06T00:02:00Z").unwrap();
        assert_eq!(n, 1);
        let runs = store.list_runs().unwrap();
        assert_eq!(runs[0].status, "interrupted");
        assert_eq!(runs[0].finished_at.as_deref(), Some("2026-07-06T00:02:00Z"));
    }
}
