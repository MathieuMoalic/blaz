import 'dart:convert';
import 'package:http/http.dart' as http;

/// Compile-time base URL (override with --dart-define API_BASE_URL=...)
const String _baseUrl = String.fromEnvironment(
  'API_BASE_URL',
  defaultValue: 'http://127.0.0.1:8080',
);

// ---------- Models ----------

class MealPlanEntry {
  final int id;
  final String day; // yyyy-MM-dd
  final int recipeId;
  final String title; // recipe title
  MealPlanEntry({
    required this.id,
    required this.day,
    required this.recipeId,
    required this.title,
  });
  factory MealPlanEntry.fromJson(Map<String, dynamic> j) => MealPlanEntry(
    id: (j['id'] as num).toInt(),
    day: j['day'] as String,
    recipeId: (j['recipe_id'] as num).toInt(),
    title: j['title'] as String,
  );
}

class ShoppingItem {
  final int id;
  final String text;
  final bool done;
  ShoppingItem({required this.id, required this.text, required this.done});
  factory ShoppingItem.fromJson(Map<String, dynamic> j) => ShoppingItem(
    id: (j['id'] as num).toInt(),
    text: j['text'] as String,
    done: (j['done'] as num).toInt() != 0, // backend returns 0/1
  );
  Map<String, dynamic> toJson() => {'id': id, 'text': text, 'done': done};
}

// ---------- Helpers ----------
Uri _u(String path, [Map<String, dynamic>? q]) => Uri.parse(
  '$_baseUrl$path',
).replace(queryParameters: q?.map((k, v) => MapEntry(k, '$v')));

Never _throw(http.Response r) =>
    throw Exception('HTTP ${r.statusCode} ${r.request?.url}: ${r.body}');

// ---------- Recipes ----------
class Recipe {
  final int id;
  final String title;
  final String source;
  final String
  yieldText; // "yield" is a Dart keyword in some contexts, avoid clash
  final String notes;
  final String createdAt; // raw string from backend (SQLite CURRENT_TIMESTAMP)
  final String updatedAt;
  final List<String> ingredients;
  final List<String> instructions;

  Recipe({
    required this.id,
    required this.title,
    required this.source,
    required this.yieldText,
    required this.notes,
    required this.createdAt,
    required this.updatedAt,
    required this.ingredients,
    required this.instructions,
  });

  factory Recipe.fromJson(Map<String, dynamic> j) => Recipe(
    id: (j['id'] ?? 0) as int,
    title: (j['title'] ?? '') as String,
    source: (j['source'] ?? '') as String,
    yieldText: (j['yield'] ?? '') as String,
    notes: (j['notes'] ?? '') as String,
    createdAt: (j['created_at'] ?? '') as String,
    updatedAt: (j['updated_at'] ?? '') as String,
    ingredients: ((j['ingredients'] as List?) ?? const [])
        .map((e) => e.toString())
        .toList(),
    instructions: ((j['instructions'] as List?) ?? const [])
        .map((e) => e.toString())
        .toList(),
  );
}

Future<List<Recipe>> fetchRecipes() async {
  final res = await http.get(Uri.parse('$_baseUrl/recipes'));
  if (res.statusCode != 200) {
    throw Exception('HTTP ${res.statusCode}: ${res.body}');
  }
  final List data = jsonDecode(res.body) as List;
  return data.map((e) => Recipe.fromJson(e as Map<String, dynamic>)).toList();
}

Future<Recipe> fetchRecipe(int id) async {
  final res = await http.get(Uri.parse('$_baseUrl/recipes/$id'));
  if (res.statusCode != 200) {
    throw Exception('HTTP ${res.statusCode}: ${res.body}');
  }
  return Recipe.fromJson(jsonDecode(res.body) as Map<String, dynamic>);
}

Future<Recipe> createRecipeFull({
  required String title,
  String source = '',
  String yieldText = '',
  String notes = '',
  List<String> ingredients = const [],
  List<String> instructions = const [],
}) async {
  final body = jsonEncode({
    'title': title,
    'source': source,
    'yield': yieldText, // backend expects "yield"
    'notes': notes,
    'ingredients': ingredients,
    'instructions': instructions,
  });

  final res = await http.post(
    Uri.parse('$_baseUrl/recipes'),
    headers: {'content-type': 'application/json'},
    body: body,
  );
  if (res.statusCode != 200 && res.statusCode != 201) {
    throw Exception('HTTP ${res.statusCode}: ${res.body}');
  }
  return Recipe.fromJson(jsonDecode(res.body) as Map<String, dynamic>);
}

Future<Recipe> getRecipe(int id) async {
  final r = await http.get(_u('/recipes/$id'));
  if (r.statusCode != 200) _throw(r);
  return Recipe.fromJson(jsonDecode(r.body) as Map<String, dynamic>);
}

Future<void> deleteRecipe(int id) async {
  final res = await http.delete(Uri.parse('$_baseUrl/recipes/$id'));
  if (res.statusCode != 204) {
    throw Exception('HTTP ${res.statusCode}: ${res.body}');
  }
}

// ---------- Meal plan ----------
Future<List<MealPlanEntry>> fetchMealPlanForDay(String day) async {
  final r = await http.get(_u('/meal-plan', {'day': day}));
  if (r.statusCode != 200) _throw(r);
  final List data = jsonDecode(r.body) as List;
  return data
      .map((e) => MealPlanEntry.fromJson(e as Map<String, dynamic>))
      .toList();
}

Future<MealPlanEntry> assignRecipeToDay({
  required String day,
  required int recipeId,
}) async {
  final r = await http.post(
    _u('/meal-plan'),
    headers: {'content-type': 'application/json'},
    body: jsonEncode({'day': day, 'recipe_id': recipeId}),
  );
  if (r.statusCode != 200) _throw(r);
  return MealPlanEntry.fromJson(jsonDecode(r.body) as Map<String, dynamic>);
}

Future<void> unassignRecipeFromDay({
  required String day,
  required int recipeId,
}) async {
  final r = await http.delete(_u('/meal-plan/$day/$recipeId'));
  if (r.statusCode != 200) _throw(r);
}

// ---------- Shopping ----------
Future<List<ShoppingItem>> fetchShoppingList() async {
  final r = await http.get(_u('/shopping'));
  if (r.statusCode != 200) _throw(r);
  final List data = jsonDecode(r.body) as List;
  return data
      .map((e) => ShoppingItem.fromJson(e as Map<String, dynamic>))
      .toList();
}

Future<ShoppingItem> createShoppingItem(String text) async {
  final r = await http.post(
    _u('/shopping'),
    headers: {'content-type': 'application/json'},
    body: jsonEncode({'text': text}),
  );
  if (r.statusCode != 200) _throw(r);
  return ShoppingItem.fromJson(jsonDecode(r.body) as Map<String, dynamic>);
}

Future<ShoppingItem> toggleShoppingItem({
  required int id,
  required bool done,
}) async {
  final r = await http.patch(
    _u('/shopping/$id'),
    headers: {'content-type': 'application/json'},
    body: jsonEncode({'done': done}),
  );
  if (r.statusCode != 200) _throw(r);
  return ShoppingItem.fromJson(jsonDecode(r.body) as Map<String, dynamic>);
}

Future<void> deleteShoppingItem(int id) async {
  final r = await http.delete(_u('/shopping/$id'));
  if (r.statusCode != 200) _throw(r);
}
