use serde::Deserialize;
use std::time::Duration;

use crate::llm::LlmClient;
use crate::models::AppState;
use crate::units::normalize_name;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Other,
    Fruits,
    Vegetables,
    Bakery,
    Vegan,
    Drinks,
    Alcohol,
    Seasoning,
    Canned,
    Pantry,
    NonFood,
    Pharmacy,
    Online,
    OnlineAlcohol,
}

impl Category {
    pub const ALL: &'static [Self] = &[
        Self::Other,
        Self::Fruits,
        Self::Vegetables,
        Self::Bakery,
        Self::Vegan,
        Self::Drinks,
        Self::Alcohol,
        Self::Seasoning,
        Self::Canned,
        Self::Pantry,
        Self::NonFood,
        Self::Pharmacy,
        Self::Online,
        Self::OnlineAlcohol,
    ];

    pub const fn sort_key(self) -> u8 {
        match self {
            Self::Other => 0,
            Self::Fruits => 1,
            Self::Vegetables => 2,
            Self::Bakery => 3,
            Self::Vegan => 4,
            Self::Drinks => 5,
            Self::Alcohol => 6,
            Self::Seasoning => 7,
            Self::Canned => 8,
            Self::Pantry => 9,
            Self::NonFood => 10,
            Self::Pharmacy => 11,
            Self::Online => 12,
            Self::OnlineAlcohol => 13,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Other => "Other",
            Self::Fruits => "Fruits",
            Self::Vegetables => "Vegetables",
            Self::Bakery => "Bakery",
            Self::Vegan => "Vegan",
            Self::Drinks => "Drinks",
            Self::Alcohol => "Alcohol",
            Self::Seasoning => "Seasoning",
            Self::Canned => "Canned",
            Self::Pantry => "Pantry",
            Self::NonFood => "Non-Food",
            Self::Pharmacy => "Pharmacy",
            Self::Online => "Online",
            Self::OnlineAlcohol => "Online Alcohol",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "Other" => Self::Other,
            "Fruits" => Self::Fruits,
            "Vegetables" => Self::Vegetables,
            "Bakery" => Self::Bakery,
            "Vegan" => Self::Vegan,
            "Drinks" => Self::Drinks,
            "Alcohol" => Self::Alcohol,
            "Seasoning" => Self::Seasoning,
            "Canned" => Self::Canned,
            "Pantry" => Self::Pantry,
            "Non-Food" => Self::NonFood,
            "Pharmacy" => Self::Pharmacy,
            "Online" => Self::Online,
            "Online Alcohol" => Self::OnlineAlcohol,
            _ => return None,
        })
    }
}

/* =========================
 * LLM category classifier
 * ========================= */

fn build_llm_system_prompt() -> String {
    let cats = Category::ALL
        .iter()
        .map(|c| c.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "You are a strict shopping-item category classifier.\n\
         Your job is to map a single shopping item name to EXACTLY ONE category.\n\n\
         Allowed categories (case-sensitive strings): {cats}\n\n\
         Return STRICT JSON with exactly this shape:\n\
         {{\"category\": \"<one of the allowed categories>\"}}\n\n\
         Rules:\n\
         - Do NOT invent new categories.\n\
         - If unsure, choose \"Other\".\n\
         - The item name can be in any language.\n\
         - Do not include commentary."
    )
}

#[derive(Deserialize)]
struct LlmCatOut {
    category: String,
}

pub async fn guess_category(state: &AppState, name_raw: &str) -> String {
    let fallback = || Category::Other.as_str().to_string();

    // Grab what we need from settings, then drop the lock before awaits.
    let (token, base, model) = {
        let st = state.settings.read().await;
        (
            st.llm_api_key.clone().unwrap_or_default(),
            st.llm_api_url.clone(),
            st.llm_model.clone(),
        )
    };

    if token.trim().is_empty() {
        return fallback();
    }

    let Ok(http) = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
    else {
        return fallback();
    };

    let llm = LlmClient::new(base, token, model);
    let system = build_llm_system_prompt();

    let user = format!(
        "Item: {raw}\nNormalized: {norm}\n\nChoose one allowed category.",
        raw = name_raw.trim(),
        norm = normalize_name(name_raw),
    );

    let Ok(val) = llm
        .chat_json(
            &http,
            &system,
            &user,
            0.0,
            Duration::from_secs(12),
            Some(120),
        )
        .await
    else {
        return fallback();
    };

    let parsed: LlmCatOut = match serde_json::from_value(val) {
        Ok(p) => p,
        Err(_) => return fallback(),
    };

    Category::from_str(&parsed.category)
        .unwrap_or(Category::Other)
        .as_str()
        .to_string()
}
