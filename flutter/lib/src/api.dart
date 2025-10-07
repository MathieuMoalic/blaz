import 'dart:convert';
import 'package:http/http.dart' as http;
import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:http_parser/http_parser.dart';
import 'package:mime/mime.dart';
import 'package:path/path.dart' as p;

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

String? mediaUrl(String? rel) {
  if (rel == null || rel.isEmpty) return null;
  final base = _baseUrl.replaceAll(RegExp(r'/+$'), '');
  final path = rel.startsWith('/') ? rel.substring(1) : rel;
  return '$base/media/$path';
}

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
  final String? imagePathSmall;
  final String? imagePathFull;

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
    this.imagePathSmall,
    this.imagePathFull,
  });

  factory Recipe.fromJson(Map<String, dynamic> j) {
    return Recipe(
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
      imagePathSmall: (j['image_path_small'] as String?) ?? '',
      imagePathFull: (j['image_path_full'] as String?) ?? '',
    );
  }
}

Future<Recipe> updateRecipe({
  required int id,
  String? title,
  String? source,
  String? yieldText,
  String? notes,
  List<String>? ingredients,
  List<String>? instructions,
}) async {
  final body = <String, dynamic>{
    if (title != null) 'title': title,
    if (source != null) 'source': source,
    if (yieldText != null) 'yield': yieldText,
    if (notes != null) 'notes': notes,
    if (ingredients != null) 'ingredients': ingredients,
    if (instructions != null) 'instructions': instructions,
  };
  final r = await http.patch(
    _u('/recipes/$id'),
    headers: {'content-type': 'application/json'},
    body: jsonEncode(body),
  );
  if (r.statusCode != 200) _throw(r);
  return Recipe.fromJson(jsonDecode(r.body) as Map<String, dynamic>);
}

Future<Recipe> uploadRecipeImage({
  required int id,
  String? path, // used on mobile/desktop
  List<int>? bytes, // used on web
  String? filename, // used on web (and as override)
}) async {
  final url = _u('/recipes/$id/image');
  final req = http.MultipartRequest('POST', url);

  if (kIsWeb) {
    if (bytes == null || (filename == null || filename.isEmpty)) {
      throw Exception('On web, provide bytes and filename');
    }
    final ct = lookupMimeType(filename) ?? 'application/octet-stream';
    req.files.add(
      http.MultipartFile.fromBytes(
        'image',
        bytes,
        filename: filename,
        contentType: MediaType.parse(ct),
      ),
    );
  } else {
    if (path == null) throw Exception('On non-web, provide a file path');
    final name = filename ?? p.basename(path);
    final ct = lookupMimeType(name) ?? 'application/octet-stream';
    req.files.add(
      await http.MultipartFile.fromPath(
        'image',
        path,
        filename: name,
        contentType: MediaType.parse(ct),
      ),
    );
  }

  final resp = await http.Response.fromStream(await req.send());
  if (resp.statusCode != 200) _throw(resp);
  return Recipe.fromJson(jsonDecode(resp.body) as Map<String, dynamic>);
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

/// Add multiple shopping items at once.
/// - Trims whitespace and drops empties
/// - De-dupes within the input
/// - If [avoidDuplicates] is true (default), skips items already present
///   in the current shopping list (case-insensitive text match)
/// - Returns the list of successfully created items
/// - Throws if any item fails to create (none are rolled back)
Future<List<ShoppingItem>> addShoppingItems(
  List<String> items, {
  bool avoidDuplicates = true,
}) async {
  // Clean & de-dupe input (case-insensitive)
  final cleaned = <String>[];
  final seen = <String>{};
  for (final raw in items) {
    final s = raw.trim();
    if (s.isEmpty) continue;
    final key = s.toLowerCase();
    if (seen.add(key)) cleaned.add(s);
  }

  // Optionally skip ones already in the shopping list
  List<String> toCreate = List.of(cleaned);
  if (avoidDuplicates) {
    final existing = await fetchShoppingList();
    final existingKeys = existing
        .map((e) => e.text.trim().toLowerCase())
        .toSet();
    toCreate.removeWhere((s) => existingKeys.contains(s.trim().toLowerCase()));
  }

  if (toCreate.isEmpty) return <ShoppingItem>[];

  final created = <ShoppingItem>[];
  final failures = <String>[];

  // Create sequentially (simple + avoids spamming backend)
  for (final text in toCreate) {
    try {
      final item = await createShoppingItem(text);
      created.add(item);
    } catch (e) {
      failures.add('$text ($e)');
    }
  }

  if (failures.isNotEmpty) {
    // Some (or all) failed — surface a clear error.
    // Callers can still rely on the returned list when no exception is thrown.
    throw Exception(
      'Failed to add ${failures.length} item(s): ${failures.join(', ')}',
    );
  }

  return created;
}
