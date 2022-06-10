// Copyright (c) 2022 IBM
//
// SPDX-License-Identifier: Apache-2.0
//

use crate::policy;
use crate::request;

use anyhow::*;
use std::env;
use std::result::Result::Ok;
use uuid::Uuid;

use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};
use sqlx::any::{AnyKind, AnyPoolOptions};
use sqlx::AnyPool;
use sqlx::Row;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    pub policy: u32,
    pub fw_api_major: u32,
    pub fw_api_minor: u32,
    pub fw_build_id: u32,
    pub launch_description: String,
    pub fw_digest: String,
}

impl Default for Connection {
    fn default() -> Connection {
        Connection {
            policy: 0x0,
            fw_api_major: 0,
            fw_api_minor: 0,
            fw_build_id: 14,
            launch_description: "test".to_string(),
            fw_digest: "placeholder".to_string(),
        }
    }
}

pub async fn get_dbpool() -> Result<AnyPool> {
    let db_type = env::var("KBS_DB_TYPE").expect("KBS_DB_TYPE not set");
    let host_name = env::var("KBS_DB_HOST").expect("KBS_DB_HOST not set");
    let user_name = env::var("KBS_DB_USER").expect("KBS_DB_USER not set.");
    let db_pw = env::var("KBS_DB_PW").expect("KBS_DB_PW not set.");
    let data_base = env::var("KBS_DB").expect("KBS_DB not set");

    let db_url = if db_type == "sqlite" {
        format!("{}://{}", db_type, data_base)
    } else {
        format!(
            "{}://{}:{}@{}/{}",
            db_type, user_name, db_pw, host_name, data_base
        )
    };

    let db_pool = AnyPoolOptions::new()
        .max_connections(1000)
        .connect(&db_url)
        .await
        .map_err(|e| {
            anyhow!(
                "db::get_db_pool:: Encountered error trying to create database pool: {}",
                e
            )
        })?;
    Ok(db_pool)
}

fn replace_binds(kind: AnyKind, sql: &str) -> String {
    if kind != AnyKind::Postgres {
        return sql.to_string();
    }

    // Replace question marks by $1, $2, ...
    let question_mark_re = Regex::new(r"\?").unwrap();
    let mut counter = 0;
    let result = question_mark_re.replace_all(sql, |_: &Captures| {
        counter += 1;
        format!("${}", counter)
    });
    result.to_string()
}

pub async fn insert_connection(connection: Connection) -> Result<Uuid> {
    let nwuuid = Uuid::new_v4();
    let uuidstr = nwuuid.as_hyphenated().to_string();

    let dbpool = get_dbpool().await?;

    let db_type = env::var("KBS_DB_TYPE").expect("KBS_DB_TYPE not set");
    let query_str = if db_type == "sqlite" {
        "INSERT INTO conn_bundle (id, policy, fw_api_major, fw_api_minor, fw_build_id, launch_description, fw_digest, create_date) VALUES (?, ?, ?, ?, ?, ?, ?, DATE('now'))"
    } else {
        "INSERT INTO conn_bundle (id, policy, fw_api_major, fw_api_minor, fw_build_id, launch_description, fw_digest, create_date) VALUES (?, ?, ?, ?, ?, ?, ?, NOW())"
    };

    let new_query_str = replace_binds(dbpool.any_kind(), query_str);

    sqlx::query(&new_query_str)
        .bind(uuidstr)
        .bind(connection.policy as i64)
        .bind(connection.fw_api_major as i64)
        .bind(connection.fw_api_minor as i64)
        .bind(connection.fw_build_id as i64)
        .bind(&connection.launch_description)
        .bind(&connection.fw_digest)
        .execute(&dbpool)
        .await?;
    Ok(nwuuid)
}

pub async fn get_connection(uuid: Uuid) -> Result<Connection> {
    let uuidstr = uuid.as_hyphenated().to_string();

    let dbpool = get_dbpool().await?;

    let query_str = "SELECT policy, fw_api_major, fw_api_minor, fw_build_id, launch_description, fw_digest FROM conn_bundle WHERE id = ?";
    let new_query_str = replace_binds(dbpool.any_kind(), query_str);

    let con_row = sqlx::query(&new_query_str)
        .bind(uuidstr)
        .fetch_one(&dbpool)
        .await?;
    Ok(Connection {
        policy: con_row.try_get::<i32, _>(0)? as u32,
        fw_api_major: con_row.try_get::<i32, _>(1)? as u32,
        fw_api_minor: con_row.try_get::<i32, _>(2)? as u32,
        fw_build_id: con_row.try_get::<i32, _>(3)? as u32,
        launch_description: con_row.try_get::<String, _>(4)?,
        fw_digest: con_row.try_get::<String, _>(5)?,
    })
}

pub async fn delete_connection(uuid: Uuid) -> Result<Uuid> {
    let uuidstr = uuid.as_hyphenated().to_string();

    let dbpool = get_dbpool().await?;

    let query_str = "DELETE from conn_bundle WHERE id = ?";
    let new_query_str = replace_binds(dbpool.any_kind(), query_str);

    sqlx::query(&new_query_str)
        .bind(uuidstr)
        .execute(&dbpool)
        .await?;
    Ok(uuid)
}

pub async fn insert_policy(policy: &policy::Policy) -> Result<u64> {
    let allowed_digests_json = serde_json::to_string(&policy.allowed_digests)?;
    let allowed_policies_json = serde_json::to_string(&policy.allowed_policies)?;
    let allowed_build_ids_json = serde_json::to_string(&policy.allowed_build_ids)?;

    let dbpool = get_dbpool().await?;

    let db_type = env::var("KBS_DB_TYPE").expect("KBS_DB_TYPE not set");
    let mut query_str = if db_type == "sqlite" {
        String::from("INSERT INTO policy (allowed_digests, allowed_policies, min_fw_api_major, min_fw_api_minor, allowed_build_ids, create_date, valid) VALUES(?, ?, ?, ?, ?, DATE('now'), 1)")
    } else {
        String::from("INSERT INTO policy (allowed_digests, allowed_policies, min_fw_api_major, min_fw_api_minor, allowed_build_ids, create_date, valid) VALUES(?, ?, ?, ?, ?, NOW(), 1)")
    };

    if dbpool.any_kind() == AnyKind::MySql || dbpool.any_kind() == AnyKind::Sqlite {
        let last_insert_row = sqlx::query(&query_str)
            .bind(allowed_digests_json)
            .bind(allowed_policies_json)
            .bind(policy.min_fw_api_major as i64)
            .bind(policy.min_fw_api_minor as i64)
            .bind(allowed_build_ids_json)
            .execute(&dbpool)
            .await?
            .last_insert_id();
        match last_insert_row {
            Some(p) => Ok(p as u64),
            None => Err(anyhow!(
                "db::insert_policy- error, last_insert_id() returned None"
            )),
        }
    } else {
        query_str.push_str("RETURNING id");
        let new_query_str = replace_binds(dbpool.any_kind(), &query_str);
        let last_insert_row = sqlx::query(&new_query_str)
            .bind(allowed_digests_json)
            .bind(allowed_policies_json)
            .bind(policy.min_fw_api_major as i64)
            .bind(policy.min_fw_api_minor as i64)
            .bind(allowed_build_ids_json)
            .fetch_one(&dbpool)
            .await?;
        Ok(last_insert_row.try_get::<i32, _>(0)? as u64)
    }
}

pub async fn get_policy(pid: u64) -> Result<policy::Policy> {
    let dbpool = get_dbpool().await?;

    let query_str = "SELECT allowed_digests, allowed_policies, min_fw_api_major, min_fw_api_minor, allowed_build_ids FROM policy WHERE id = ? AND valid = 1";
    let new_query_str = replace_binds(dbpool.any_kind(), query_str);

    let policy_row = sqlx::query(&new_query_str)
        .bind(pid as i64)
        .fetch_one(&dbpool)
        .await?;

    Ok(policy::Policy {
        allowed_digests: serde_json::from_str(&policy_row.try_get::<String, _>(0)?)?,
        allowed_policies: serde_json::from_str(&policy_row.try_get::<String, _>(1)?)?,
        min_fw_api_major: policy_row.try_get::<i32, _>(2)? as u32,
        min_fw_api_minor: policy_row.try_get::<i32, _>(3)? as u32,
        allowed_build_ids: serde_json::from_str(&policy_row.try_get::<String, _>(4)?)?,
    })
}

pub async fn delete_policy(pid: u64) -> Result<()> {
    let dbpool = get_dbpool().await?;

    let query_str = "DELETE from policy WHERE id = ?";
    let new_query_str = replace_binds(dbpool.any_kind(), query_str);

    sqlx::query(&new_query_str)
        .bind(pid as i64)
        .execute(&dbpool)
        .await?;
    Ok(())
}

pub async fn get_secret_policy(sec: &str) -> Result<policy::Policy> {
    let dbpool = get_dbpool().await?;

    let query_str = "SELECT polid FROM secrets WHERE secret_id = ?";
    let new_query_str = replace_binds(dbpool.any_kind(), query_str);

    let pol_row = sqlx::query(&new_query_str)
        .bind(sec)
        .fetch_one(&dbpool)
        .await?;
    let pol = pol_row.try_get::<i64, _>(0)? as u64;
    let secret_policy = get_policy(pol).await?;
    Ok(secret_policy)
}

pub async fn insert_keyset(ksetid: &str, kskeys: &[String], polid: Option<u32>) -> Result<()> {
    let kskeys_str = serde_json::to_string(kskeys)?;

    let dbpool = get_dbpool().await?;

    match polid {
        Some(p) => {
            let query_str = "INSERT INTO keysets (keysetid, kskeys, polid) VALUES(?, ?, ?)";
            let new_query_str = replace_binds(dbpool.any_kind(), query_str);
            sqlx::query(&new_query_str)
                .bind(ksetid)
                .bind(&kskeys_str)
                .bind(p as i64)
                .execute(&dbpool)
                .await?;
            Ok(())
        }
        None => {
            let query_str = "INSERT INTO keysets (keysetid, kskeys) VALUES(?, ?)";
            let new_query_str = replace_binds(dbpool.any_kind(), query_str);
            sqlx::query(&new_query_str)
                .bind(ksetid)
                .bind(&kskeys_str)
                .execute(&dbpool)
                .await?;
            Ok(())
        }
    }
}

pub async fn delete_keyset(ksetid: &str) -> Result<()> {
    let dbpool = get_dbpool().await?;

    let query_str = "DELETE from keysets WHERE keysetid = ?";
    let new_query_str = replace_binds(dbpool.any_kind(), query_str);

    sqlx::query(&new_query_str)
        .bind(ksetid)
        .execute(&dbpool)
        .await?;
    Ok(())
}

pub async fn get_keyset_policy(keysetid: &str) -> Result<policy::Policy> {
    let dbpool = get_dbpool().await?;

    let query_str = "SELECT polid FROM keysets WHERE keysetid = ?";
    let new_query_str = replace_binds(dbpool.any_kind(), query_str);

    let pol_row = sqlx::query(&new_query_str)
        .bind(keysetid)
        .fetch_one(&dbpool)
        .await?;
    let pol = pol_row.try_get::<i64, _>(0)? as u64;
    let secret_policy = get_policy(pol).await?;
    Ok(secret_policy)
}

pub async fn get_keyset_ids(keysetid: &str) -> Result<Vec<String>> {
    let dbpool = get_dbpool().await?;

    let query_str = "SELECT kskeys FROM keysets WHERE keysetid = ?";
    let new_query_str = replace_binds(dbpool.any_kind(), query_str);

    let keyset_row = sqlx::query(&new_query_str)
        .bind(keysetid)
        .fetch_one(&dbpool)
        .await?;
    let rks: Vec<String> = serde_json::from_str(&keyset_row.try_get::<String, _>(0)?).unwrap();
    Ok(rks)
}

pub async fn get_secret(secret_id: &str) -> Result<request::Key> {
    let dbpool = get_dbpool().await?;

    let query_str = "SELECT secret FROM secrets WHERE secret_id = ?";
    let new_query_str = replace_binds(dbpool.any_kind(), query_str);

    let secret_row = sqlx::query(&new_query_str)
        .bind(secret_id)
        .fetch_one(&dbpool)
        .await?;
    let secret = secret_row.try_get::<String, _>(0)?;
    Ok(request::Key {
        id: secret_id.to_string(),
        payload: secret,
    })
}

pub async fn insert_secret(secret_id: &str, secret: &str, policy_id: Option<u64>) -> Result<()> {
    let dbpool = get_dbpool().await?;
    let query_str = "INSERT INTO secrets (secret_id, secret, polid ) VALUES(?, ?, ?)";
    let new_query_str = replace_binds(dbpool.any_kind(), query_str);
    sqlx::query(&new_query_str)
        .bind(secret_id)
        .bind(secret)
        .bind(policy_id.map(|p| p as i64))
        .execute(&dbpool)
        .await?;
    Ok(())
}

pub async fn delete_secret(secret_id: &str) -> Result<()> {
    let dbpool = get_dbpool().await?;

    let query_str = "DELETE from secrets WHERE secret_id = ?";
    let new_query_str = replace_binds(dbpool.any_kind(), query_str);

    sqlx::query(&new_query_str)
        .bind(secret_id)
        .execute(&dbpool)
        .await?;
    Ok(())
}

pub async fn insert_report_keypair(id: &str, keypair: &[u8], policy_id: Option<u64>) -> Result<()> {
    let keypair_b64 = base64::encode(&keypair);

    let dbpool = get_dbpool().await?;
    let query_str = "INSERT INTO report_keypair (key_id, keypair, polid ) VALUES(?, ?, ?)";
    let new_query_str = replace_binds(dbpool.any_kind(), query_str);
    sqlx::query(&new_query_str)
        .bind(id)
        .bind(&keypair_b64)
        .bind(policy_id.map(|p| p as i64))
        .execute(&dbpool)
        .await?;
    Ok(())
}

pub async fn get_report_keypair(id: &str) -> Result<Vec<u8>> {
    let dbpool = get_dbpool().await?;

    let query_str = "SELECT keypair FROM report_keypair WHERE key_id = ?";
    let new_query_str = replace_binds(dbpool.any_kind(), query_str);

    let key_row = sqlx::query(&new_query_str)
        .bind(id)
        .fetch_one(&dbpool)
        .await?;
    let kp = key_row.try_get::<String, _>(0)?;
    let kp_bytes = base64::decode(&kp)?;
    Ok(kp_bytes)
}

pub async fn delete_report_keypair(key_id: &str) -> Result<()> {
    let dbpool = get_dbpool().await?;

    let query_str = "DELETE from report_keypair WHERE key_id = ?";
    let new_query_str = replace_binds(dbpool.any_kind(), query_str);

    sqlx::query(&new_query_str)
        .bind(key_id)
        .execute(&dbpool)
        .await?;
    Ok(())
}

pub async fn get_signing_keys_policy(key_id: &str) -> Result<Option<policy::Policy>> {
    let dbpool = get_dbpool().await?;

    let query_str = "SELECT polid FROM report_keypair WHERE key_id = ? AND polid IS NOT NULL";
    let new_query_str = replace_binds(dbpool.any_kind(), query_str);
    let policy_id_option = sqlx::query(&new_query_str)
        .bind(key_id)
        .fetch_optional(&dbpool)
        .await?;
    match policy_id_option {
        Some(p) => {
            let pid = p.try_get::<i64, _>(0)? as u64;
            Ok(Some(get_policy(pid).await?))
        }
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[tokio::test]
    async fn test_connection() -> anyhow::Result<()> {
        let testconn = Connection::default();

        let tid = insert_connection(testconn.clone()).await?;

        let resconn = get_connection(tid.clone()).await?;

        assert_eq!(testconn.policy, resconn.policy);
        assert_eq!(testconn.fw_api_major, resconn.fw_api_major);
        assert_eq!(testconn.fw_api_minor, resconn.fw_api_minor);
        assert_eq!(testconn.fw_build_id, resconn.fw_build_id);
        assert_eq!(testconn.launch_description, resconn.launch_description);
        let _dconid = delete_connection(tid.clone()).await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_insert_policy() -> anyhow::Result<()> {
        let testpol = policy::Policy {
            allowed_digests: vec!["0".to_string(), "1".to_string(), "3".to_string()],
            allowed_policies: vec![0u32, 1u32, 2u32],
            min_fw_api_major: 0,
            min_fw_api_minor: 0,
            allowed_build_ids: vec![0u32, 1u32, 2u32],
        };

        let polid = insert_policy(&testpol).await?;

        let rpol = get_policy(polid).await?;

        for j in 0..2 {
            assert_eq!(
                rpol.allowed_digests[j].clone(),
                testpol.allowed_digests[j].clone()
            );
        }

        for j in 0..2 {
            assert_eq!(
                rpol.allowed_policies[j].clone(),
                testpol.allowed_policies[j].clone()
            );
        }

        assert_eq!(rpol.min_fw_api_major.clone(), 0);
        assert_eq!(rpol.min_fw_api_minor.clone(), 0);

        for j in 0..2 {
            assert_eq!(
                rpol.allowed_build_ids[j].clone(),
                testpol.allowed_build_ids[j].clone()
            );
        }

        delete_policy(polid).await?;
        Ok(())
    }

    //#[test]
    #[tokio::test]
    async fn test_secret_policy() -> anyhow::Result<()> {
        let tinspol = policy::Policy {
            allowed_digests: vec![
                "PuBT5e0dD21ZDoqdiBMNjWeKV2WhtcEOIdWeEsFwivw=".to_string(),
                "1".to_owned(),
                "3".to_owned(),
            ],
            allowed_policies: vec![0u32, 1u32, 2u32],
            min_fw_api_major: 23,
            min_fw_api_minor: 0,
            allowed_build_ids: vec![0u32, 1u32, 2u32],
        };

        let tpid = insert_policy(&tinspol).await?;

        let secid_uuid = Uuid::new_v4().as_hyphenated().to_string();
        let sec_uuid = Uuid::new_v4().as_hyphenated().to_string();

        insert_secret(&secid_uuid, &sec_uuid, Option::Some(tpid)).await?;

        let testpol = get_secret_policy(&secid_uuid).await?;

        assert_eq!(
            testpol.allowed_digests[0],
            "PuBT5e0dD21ZDoqdiBMNjWeKV2WhtcEOIdWeEsFwivw="
        );
        assert_eq!(testpol.allowed_policies[0], 0);
        assert_eq!(testpol.min_fw_api_major, 23);
        assert_eq!(testpol.min_fw_api_minor, 0);
        assert_eq!(testpol.allowed_build_ids[0], 0);

        delete_secret(&secid_uuid).await?;
        delete_policy(tpid).await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_secrets() -> anyhow::Result<()> {
        let secid = Uuid::new_v4().as_hyphenated().to_string();
        let sec = Uuid::new_v4().as_hyphenated().to_string();
        let polid = 0u64;
        insert_secret(&secid, &sec, Some(polid)).await?;

        let tkey = get_secret(&secid).await?;

        assert_eq!(tkey.id, secid);
        assert_eq!(tkey.payload, sec);

        delete_secret(&secid).await?;
        Ok(())
    }

    //#[test]
    #[tokio::test]
    async fn test_insert_keyset() -> anyhow::Result<()> {
        let keys: Vec<String> = vec![
            "RGlyZSBXb2xmCg==".to_string(),
            "VGhlIFJhY2UgaXMgT24K".into(),
            "T2ggQmFiZSwgSXQgQWluJ3QgTm8gTGllCg==".into(),
            "SXQgTXVzdCBIYXZlIEJlZW4gdGhlIFJvc2VzCg==".into(),
            "RGFyayBIb2xsb3cK".into(),
            "Q2hpbmEgRG9sbAo=".into(),
            "QmVlbiBBbGwgQXJvdW5kIFRoaXMgV29ybGQK".into(),
            "TW9ua2V5IGFuZCB0aGUgRW5naW5lZXIK".into(),
            "SmFjay1BLVJvZQo=".into(),
            "RGVlcCBFbGVtIEJsdWVzCg==".into(),
            "Q2Fzc2lkeQo=".into(),
            "VG8gTGF5IE1lIERvd24K".into(),
            "Um9zYWxpZSBNY0ZhbGwK".into(),
            "T24gdGhlIFJvYWQgQWdhaW4K".into(),
            "QmlyZCBTb25nCg==".into(),
            "UmlwcGxlCg==".into(),
        ];

        let ksetid = Uuid::new_v4().as_hyphenated().to_string();
        let polid = Some(1u32);
        insert_keyset(&ksetid, &keys, polid).await?;

        let keyset_ids = get_keyset_ids(&ksetid).await?;
        assert_eq!(keyset_ids.len(), keys.len());
        assert_eq!(keyset_ids, keys);

        delete_keyset(&ksetid).await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_get_report_keypair() -> anyhow::Result<()> {
        let tid = "man-moon-dog-face-in-the-banana-patch".to_string();

        let pkcs8_dummy_bytes = [0xa5u8; 512];

        insert_report_keypair(&tid, &pkcs8_dummy_bytes, None)
            .await
            .unwrap();

        let keypair_vec = get_report_keypair(&tid).await.unwrap();
        assert_eq!(keypair_vec, &pkcs8_dummy_bytes);

        delete_report_keypair(&tid).await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_get_signing_keys_policy() -> anyhow::Result<()> {
        let testpol = policy::Policy {
            allowed_digests: vec!["0".to_string(), "1".to_string(), "3".to_string()],
            allowed_policies: vec![0u32, 1u32, 2u32],
            min_fw_api_major: 0,
            min_fw_api_minor: 0,
            allowed_build_ids: vec![0u32, 1u32, 2u32],
        };

        let polid = insert_policy(&testpol).await?;

        let mut tid = "man-moon-dog-face-in-the-banana-patch-ksp".to_string();

        let pkcs8_dummy_bytes = [0xa5u8; 512];

        // First test with valid policy id

        insert_report_keypair(&tid, &pkcs8_dummy_bytes, Option::Some(polid))
            .await
            .unwrap();
        let keypair_policy = get_signing_keys_policy(&tid).await?;
        assert_eq!(keypair_policy, Option::Some(testpol));
        delete_report_keypair(&tid).await?;
        delete_policy(polid).await?;

        // Now test report_keypair without a policy

        tid = "the-quick-brown-cow-jumped-over-the-moon-no-policy".to_string();

        insert_report_keypair(&tid, &pkcs8_dummy_bytes, None).await?;

        let keypair_policy = get_signing_keys_policy(&tid).await?;
        assert_eq!(keypair_policy, None);
        delete_report_keypair(&tid).await?;

        Ok(())
    }
}
