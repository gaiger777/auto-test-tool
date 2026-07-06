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
    pub status: String, // running | passed | failed | cancelled
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
             );",
        )
        .map_err(|e| e.to_string())?;
        Ok(Self { conn })
    }

    // --- environments ---

    pub fn save_environment(&self, env: &Environment) -> Result<i64, String> {
        let endpoints = serde_json::to_string(&env.endpoints).map_err(|e| e.to_string())?;
        match env.id {
            Some(id) => {
                self.conn
                    .execute(
                        "UPDATE environments SET name=?1, keystone_url=?2, user_name=?3, user_domain=?4,
                         project_name=?5, project_domain=?6, mq_url=?7, mq_exchanges=?8, endpoints=?9 WHERE id=?10",
                        params![env.name, env.keystone_url, env.user_name, env.user_domain,
                                env.project_name, env.project_domain, env.mq_url, env.mq_exchanges, endpoints, id],
                    )
                    .map_err(|e| e.to_string())?;
                Ok(id)
            }
            None => {
                self.conn
                    .execute(
                        "INSERT INTO environments (name, keystone_url, user_name, user_domain,
                         project_name, project_domain, mq_url, mq_exchanges, endpoints)
                         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                        params![env.name, env.keystone_url, env.user_name, env.user_domain,
                                env.project_name, env.project_domain, env.mq_url, env.mq_exchanges, endpoints],
                    )
                    .map_err(|e| e.to_string())?;
                Ok(self.conn.last_insert_rowid())
            }
        }
    }

    pub fn list_environments(&self) -> Result<Vec<Environment>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, keystone_url, user_name, user_domain, project_name, project_domain, mq_url, mq_exchanges, endpoints FROM environments ORDER BY id")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                let endpoints_json: String = r.get(9)?;
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
                    endpoints: serde_json::from_str(&endpoints_json).unwrap_or_default(),
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
        let _ = e.delete_credential();
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
    }
}
