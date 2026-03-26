/// Extract structured recipe data from schema.org JSON-LD markup
use scraper::{Html, Selector};
use serde_json::Value as JsonValue;

#[derive(Debug, Clone)]
pub struct SchemaRecipe {
    pub name: String,
    pub ingredients: Vec<String>,
    pub instructions: Vec<String>,
}

/// Extract recipe data from schema.org JSON-LD in HTML
pub fn extract_schema_recipe(html: &str) -> Option<SchemaRecipe> {
    let document = Html::parse_document(html);
    let script_selector = Selector::parse(r#"script[type="application/ld+json"]"#).ok()?;

    // Find all JSON-LD script tags
    for script in document.select(&script_selector) {
        let json_text = script.text().collect::<String>();
        
        // Try to parse as JSON
        let json: JsonValue = match serde_json::from_str(&json_text) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Handle different JSON-LD formats:
        // 1. Direct Recipe object: {"@type": "Recipe", ...}
        // 2. Array with Recipe: [{"@type": "WebSite"}, {"@type": "Recipe"}]
        // 3. Graph format: {"@graph": [{"@type": "Recipe"}]}
        let recipe = if let Some(graph) = json.get("@graph").and_then(|g| g.as_array()) {
            // Handle @graph format (common with Yoast SEO)
            graph.iter().find(|item| is_recipe_type(item))
        } else if json.is_array() {
            // Handle array format
            json.as_array()?.iter().find(|item| is_recipe_type(item))
        } else if is_recipe_type(&json) {
            // Handle direct Recipe object
            Some(&json)
        } else {
            None
        };

        if let Some(recipe_data) = recipe
            && let Some(extracted) = extract_recipe_fields(recipe_data)
        {
            tracing::info!(
                "Extracted schema.org recipe: {} with {} ingredients",
                extracted.name,
                extracted.ingredients.len()
            );
            return Some(extracted);
        }
    }

    tracing::info!("No schema.org recipe found in HTML");
    None
}

fn is_recipe_type(json: &JsonValue) -> bool {
    if let Some(type_val) = json.get("@type") {
        if let Some(type_str) = type_val.as_str() {
            return type_str == "Recipe";
        }
        // Handle @type as array
        if let Some(types) = type_val.as_array() {
            return types.iter().any(|t| t.as_str() == Some("Recipe"));
        }
    }
    false
}

fn extract_recipe_fields(recipe: &JsonValue) -> Option<SchemaRecipe> {
    let name = recipe
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Imported Recipe")
        .to_string();

    // Extract ingredients (can be array of strings or array of objects)
    let ingredients = extract_ingredients(recipe)?;
    
    // Extract instructions (can be array of strings, objects with text, or HowToSection)
    let instructions = extract_instructions(recipe)?;

    Some(SchemaRecipe {
        name,
        ingredients,
        instructions,
    })
}

fn extract_ingredients(recipe: &JsonValue) -> Option<Vec<String>> {
    let ing_value = recipe.get("recipeIngredient")?;
    
    let mut ingredients = Vec::new();
    
    if let Some(arr) = ing_value.as_array() {
        for item in arr {
            if let Some(text) = item.as_str() {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    ingredients.push(trimmed.to_string());
                }
            }
        }
    }
    
    if ingredients.is_empty() {
        None
    } else {
        Some(ingredients)
    }
}

fn extract_instructions(recipe: &JsonValue) -> Option<Vec<String>> {
    let inst_value = recipe.get("recipeInstructions")?;
    
    let mut instructions = Vec::new();
    
    // Handle array of instructions
    if let Some(arr) = inst_value.as_array() {
        for item in arr {
            // Handle string
            if let Some(text) = item.as_str() {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    instructions.push(trimmed.to_string());
                }
            }
            // Handle HowToStep object
            else if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    instructions.push(trimmed.to_string());
                }
            }
            // Handle HowToSection with itemListElement
            else if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                instructions.push(format!("## {name}"));
                if let Some(steps) = item.get("itemListElement").and_then(|v| v.as_array()) {
                    for step in steps {
                        if let Some(text) = step.get("text").and_then(|v| v.as_str()) {
                            let trimmed = text.trim();
                            if !trimmed.is_empty() {
                                instructions.push(trimmed.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    // Handle single string
    else if let Some(text) = inst_value.as_str() {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            instructions.push(trimmed.to_string());
        }
    }
    
    if instructions.is_empty() {
        None
    } else {
        Some(instructions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_simple_recipe() {
        let html = r#"
            <html>
            <head>
                <script type="application/ld+json">
                {
                    "@type": "Recipe",
                    "name": "Test Recipe",
                    "recipeIngredient": [
                        "2 cups flour",
                        "1 cup water"
                    ],
                    "recipeInstructions": [
                        "Mix ingredients",
                        "Bake at 350°F"
                    ]
                }
                </script>
            </head>
            </html>
        "#;

        let recipe = extract_schema_recipe(html).unwrap();
        assert_eq!(recipe.name, "Test Recipe");
        assert_eq!(recipe.ingredients.len(), 2);
        assert_eq!(recipe.instructions.len(), 2);
    }

    #[test]
    fn test_extract_recipe_from_array() {
        let html = r#"
            <html>
            <head>
                <script type="application/ld+json">
                [{
                    "@type": "WebSite",
                    "name": "Test Site"
                },
                {
                    "@type": "Recipe",
                    "name": "Array Recipe",
                    "recipeIngredient": ["1 cup sugar"],
                    "recipeInstructions": ["Mix well"]
                }]
                </script>
            </head>
            </html>
        "#;

        let recipe = extract_schema_recipe(html).unwrap();
        assert_eq!(recipe.name, "Array Recipe");
    }

    #[test]
    fn test_extract_recipe_from_graph() {
        let html = r#"
            <html>
            <head>
                <script type="application/ld+json">
                {
                    "@context": "https://schema.org",
                    "@graph": [
                        {
                            "@type": "WebSite",
                            "name": "Test Site"
                        },
                        {
                            "@type": "Recipe",
                            "name": "Graph Recipe",
                            "recipeIngredient": ["2 cups flour", "1 tsp salt"],
                            "recipeInstructions": ["Mix", "Bake"]
                        }
                    ]
                }
                </script>
            </head>
            </html>
        "#;

        let recipe = extract_schema_recipe(html).unwrap();
        assert_eq!(recipe.name, "Graph Recipe");
        assert_eq!(recipe.ingredients.len(), 2);
        assert_eq!(recipe.instructions.len(), 2);
    }

    #[test]
    fn test_no_recipe_returns_none() {
        let html = r#"
            <html>
            <head>
                <script type="application/ld+json">
                {
                    "@type": "Article",
                    "name": "Not a recipe"
                }
                </script>
            </head>
            </html>
        "#;

        let recipe = extract_schema_recipe(html);
        assert!(recipe.is_none());
    }
}
