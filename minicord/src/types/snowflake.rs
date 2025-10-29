use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Snowflake(String);

impl From<u64> for Snowflake {
    fn from(value: u64) -> Self {
        Self(value.to_string())
    }
}

impl TryFrom<Snowflake> for u64 {
    type Error = <Self as std::str::FromStr>::Err;

    fn try_from(value: Snowflake) -> Result<Self, Self::Error> {
        value.0.parse()
    }
}
