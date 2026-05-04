use serde::{Deserialize, Serialize};

/// User ID - globally unique across the service
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UserId(String);

impl UserId {
    #[allow(dead_code)]
    fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    #[allow(dead_code)]
    fn as_str(&self) -> &str {
        &self.0
    }
}

/// Member ID - unique within an organization
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MemberId(String);

impl MemberId {
    #[allow(dead_code)]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub id: MemberId,
    pub name: String,
    pub is_current_user: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MembersListData {
    pub members: Vec<Member>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_id_serde() {
        // シリアライズ：UserId → JSON文字列
        let user_id = UserId::new("user-123");
        let json = serde_json::to_string(&user_id).unwrap();
        assert_eq!(json, r#""user-123""#);

        // デシリアライズ：JSON文字列 → UserId
        let deserialized: UserId = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, user_id);
        assert_eq!(deserialized.as_str(), "user-123");
    }

    #[test]
    fn test_member_id_serde() {
        // シリアライズ：MemberId → JSON文字列
        let member_id = MemberId::new("member-456");
        let json = serde_json::to_string(&member_id).unwrap();
        assert_eq!(json, r#""member-456""#);

        // デシリアライズ：JSON文字列 → MemberId
        let deserialized: MemberId = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, member_id);
        assert_eq!(deserialized.as_str(), "member-456");
    }

    #[test]
    fn test_in_struct() {
        // 構造体の中でも正しく動作する
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct TestData {
            user_id: UserId,
            member_id: MemberId,
        }

        let data = TestData {
            user_id: UserId::new("user-123"),
            member_id: MemberId::new("member-456"),
        };

        let json = serde_json::to_string(&data).unwrap();
        assert_eq!(json, r#"{"user_id":"user-123","member_id":"member-456"}"#);

        let deserialized: TestData = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, data);
    }
}
