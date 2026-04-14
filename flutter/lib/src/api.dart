import 'dart:convert';
import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:http/http.dart' as http;
import 'package:http_parser/http_parser.dart';
import 'package:mime/mime.dart';
import 'package:path/path.dart' as p;

import './platform/kv_store.dart' as kv;

/// Callback invoked when a 401 response is received
void Function()? _on401;

/// Default (build-time) base URL. Can be overridden at runtime & persisted.
const String _defaultBaseUrl = String.fromEnvironment(
  'API_BASE_URL',
  defaultValue: 'https://blaz.matmoa.eu',
);

const _kApiBaseUrlKey = 'api_base_url';

String _baseUrl = _defaultBaseUrl; // mutable at runtime

/// Read-only getter so existing code that uses `api.baseUrl` keeps working.
String get baseUrl => _baseUrl;

/// Call this **before** Auth.init() (e.g., in main.dart) or inside it.
Future<void> initApi() async {
  final saved = await kv.getString(_kApiBaseUrlKey);
  if (saved != null && saved.trim().isNotEmpty) {
    _baseUrl = _normalizeBase(saved);
  } else if (kIsWeb && _baseUrl == _defaultBaseUrl) {
    // On web with no saved URL, use the page origin so share links work on
    // any self-hosted server without needing a build-time API_BASE_URL.
    final origin = Uri.base.origin;
    if (origin.isNotEmpty) _baseUrl = origin;
  }
}

/// Fetch backend version
Future<String> fetchBackendVersion() async {
  try {
    final res = await http.get(_u('/version'));
    if (res.statusCode != 200) {
      throw Exception('HTTP ${res.statusCode}: ${res.body}');
    }
    final data = jsonDecode(res.body) as Map<String, dynamic>;
    return data['version'] as String;
  } catch (e) {
    // Re-throw with more context for better error messages
    throw Exception('Failed to fetch backend version: $e');
  }
}

class CategoryOption {
  final String value;
  final String label;
  const CategoryOption(this.value, this.label);
}

const kCategoryOptions = <CategoryOption>[
  CategoryOption('Other', 'Other'),
  CategoryOption('Fruits', 'Fruits'),
  CategoryOption('Vegetables', 'Vegetables'),
  CategoryOption('Bakery', 'Bakery'),
  CategoryOption('Vegan', 'Vegan'),
  CategoryOption('Drinks', 'Drinks'),
  CategoryOption('Alcohol', 'Alcohol'),
  CategoryOption('Seasoning', 'Seasoning'),
  CategoryOption('Canned', 'Canned'),
  CategoryOption('Pantry', 'Pantry'),
  CategoryOption('Non-Food', 'Non-Food'),
  CategoryOption('Pharmacy', 'Pharmacy'),
  CategoryOption('Online', 'Online'),
  CategoryOption('Online Alcohol', 'Online Alcohol'),
];

/// Verifies `/healthz` on the provided URL, then saves & activates it.
Future<void> verifyAndSaveBaseUrl(
  String url, {
  Duration timeout = const Duration(seconds: 5),
}) async {
  final candidate = _normalizeBase(url);
  await _pingHealthz(candidate, timeout: timeout); // throws on error
  _baseUrl = candidate;
  await kv.setString(_kApiBaseUrlKey, _baseUrl);
}

/// Returns true if `/healthz` responds with 200 and body contains “ok”.
Future<bool> testBaseUrl(
  String url, {
  Duration timeout = const Duration(seconds: 5),
}) async {
  try {
    await _pingHealthz(_normalizeBase(url), timeout: timeout);
    return true;
  } catch (_) {
    return false;
  }
}

String _normalizeBase(String u) => u.replaceAll(RegExp(r'/+$'), '');

Future<void> _pingHealthz(
  String base, {
  Duration timeout = const Duration(seconds: 5),
}) async {
  final r = await http.get(Uri.parse('$base/healthz')).timeout(timeout);
  if (r.statusCode != 200 || !r.body.toLowerCase().contains('ok')) {
    throw Exception('Health check failed (${r.statusCode}): ${r.body}');
  }
}
/* =========================
 * Auth glue
 * ========================= */

String? _authToken;

void setAuthToken(String? t) {
  _authToken = t;
}

void setOn401Callback(void Function() callback) {
  _on401 = callback;
}

Map<String, String> _headers([Map<String, String>? extra, bool includeAuth = true]) {
  final h = <String, String>{};
  if (includeAuth && _authToken != null && _authToken!.isNotEmpty) {
    h['Authorization'] = 'Bearer $_authToken';
  }
  if (extra != null) h.addAll(extra);
  return h;
}

/// Splits a multi-line text field into a trimmed, non-empty list of strings.
List<String> splitLines(String s) =>
    s.split('\n').map((e) => e.trim()).where((e) => e.isNotEmpty).toList();

Future<String> login({required String password}) async {
  final r = await http.post(
    _u('/auth/login'),
    headers: _headers({'content-type': 'application/json'}),
    body: jsonEncode({'password': password}),
  );
  if (r.statusCode != 200) _throw(r);
  final data = jsonDecode(r.body) as Map<String, dynamic>;
  final token = data['token'] as String;
  setAuthToken(token); // keep it for subsequent calls
  return token;
}

/// Add multiple plain-text shopping items (one request per line).
Future<void> addShoppingItems(List<String> lines) async {
  final items = lines.map((e) => e.trim()).where((e) => e.isNotEmpty).toList();
  if (items.isEmpty) return;

  final futures = items.map(
    (t) => http.post(
      _u('/shopping'),
      headers: _headers({'content-type': 'application/json'}),
      body: jsonEncode({'text': t}),
    ),
  );

  final responses = await Future.wait(futures);
  for (final r in responses) {
    if (r.statusCode != 200) _throw(r);
  }
}

Future<ShoppingItem> updateShoppingItem({
  required int id,
  bool? done,
  String? category,
  String? notes,
  String? text,
  String? name,
  String? unit,
  double? quantity,
}) async {
  final body = <String, dynamic>{
    if (done != null) 'done': done,
    if (category != null) 'category': category,
    if (notes != null) 'notes': notes,
    if (text != null) 'text': text,
    if (name != null) 'name': name,
    if (unit != null) 'unit': unit,
    if (quantity != null) 'quantity': quantity,
  };

  final r = await http.patch(
    _u('/shopping/$id'),
    headers: _headers({'content-type': 'application/json'}),
    body: jsonEncode(body),
  );
  if (r.statusCode != 200) _throw(r);

  return ShoppingItem.fromJson(jsonDecode(r.body) as Map<String, dynamic>);
}

Uri _u(String path, [Map<String, dynamic>? q]) => Uri.parse(
  '$baseUrl$path',
).replace(queryParameters: q?.map((k, v) => MapEntry(k, '$v')));

String? mediaUrl(String? rel) {
  if (rel == null || rel.isEmpty) return null;
  final base = baseUrl.replaceAll(RegExp(r'/+$'), '');
  final path = rel.startsWith('/') ? rel.substring(1) : rel;
  return '$base/media/$path';
}

Never _throw(http.Response r) {
  if (r.statusCode == 401 && _on401 != null) {
    _on401!();
  }
  throw Exception('HTTP ${r.statusCode} ${r.request?.url}: ${r.body}');
}

/* =========================
 * Models
 * ========================= */

class IngredientMacros {
  final String name;
  final double protein; // grams
  final double fat; // grams
  final double carbs; // grams
  final bool skipped;

  const IngredientMacros({
    required this.name,
    required this.protein,
    required this.fat,
    required this.carbs,
    required this.skipped,
  });

  factory IngredientMacros.fromJson(Map<String, dynamic> j) {
    return IngredientMacros(
      name: j['name'] as String,
      protein: (j['protein_g'] as num).toDouble(),
      fat: (j['fat_g'] as num).toDouble(),
      carbs: (j['carbs_g'] as num).toDouble(),
      skipped: j['skipped'] as bool? ?? false,
    );
  }
}

class RecipeMacros {
  final double protein; // grams
  final double fat; // grams (total)
  final double carbs; // grams (excluding fiber)
  final List<IngredientMacros> ingredients;

  const RecipeMacros({
    required this.protein,
    required this.fat,
    required this.carbs,
    this.ingredients = const [],
  });

  static double _num(dynamic v) => (v as num).toDouble();

  /// Accepts either:
  /// { "macros": {"protein": 30, "fat": 20, "carbs": 50} }
  /// or flat:    {"protein": 30, "fat": 20, "carbs": 50}
  /// or *_g keys.
  factory RecipeMacros.fromAny(Map<String, dynamic> j) {
    Map<String, dynamic>? m = (j['macros'] is Map)
        ? (j['macros'] as Map).cast<String, dynamic>()
        : null;
    m ??= j;

    double read(String a, String b) {
      if (m![a] is num) return _num(m[a]);
      if (m[b] is num) return _num(m[b]);
      throw Exception('missing $a/$b');
    }

    final ingredientsList = m['ingredients'] as List?;
    final ingredients = ingredientsList != null
        ? ingredientsList
            .map((e) => IngredientMacros.fromJson(e as Map<String, dynamic>))
            .toList()
        : <IngredientMacros>[];

    return RecipeMacros(
      protein: read('protein', 'protein_g'),
      fat: read('fat', 'fat_g'),
      carbs: read('carbs', 'carbs_g'),
      ingredients: ingredients,
    );
  }

  Map<String, dynamic> toJson() => {
    'protein': protein,
    'fat': fat,
    'carbs': carbs,
  };
}

class MealPlanEntry {
  final int id;
  final String day; // yyyy-MM-dd
  final int recipeId;
  final String title;
  final String? imagePathSmall;
  MealPlanEntry({
    required this.id,
    required this.day,
    required this.recipeId,
    required this.title,
    this.imagePathSmall,
  });
  factory MealPlanEntry.fromJson(Map<String, dynamic> j) => MealPlanEntry(
    id: (j['id'] as num).toInt(),
    day: j['day'] as String,
    recipeId: (j['recipe_id'] as num).toInt(),
    title: j['title'] as String,
    imagePathSmall: j['image_path_small'] as String?,
  );
}

class PrepReminder {
  final String step;
  final int hoursBefore;

  const PrepReminder({required this.step, required this.hoursBefore});

  factory PrepReminder.fromJson(Map<String, dynamic> j) => PrepReminder(
    step: j['step'] as String,
    hoursBefore: (j['hours_before'] as num).toInt(),
  );

  Map<String, dynamic> toJson() => {'step': step, 'hours_before': hoursBefore};
}

class PrepReminderDto {  final int recipeId;
  final String recipeTitle;
  final String step;
  final int hoursBefore;
  final String dueDate;  // yyyy-MM-dd
  final String mealDate; // yyyy-MM-dd

  const PrepReminderDto({
    required this.recipeId,
    required this.recipeTitle,
    required this.step,
    required this.hoursBefore,
    required this.dueDate,
    required this.mealDate,
  });

  factory PrepReminderDto.fromJson(Map<String, dynamic> j) => PrepReminderDto(
    recipeId: (j['recipe_id'] as num).toInt(),
    recipeTitle: j['recipe_title'] as String,
    step: j['step'] as String,
    hoursBefore: (j['hours_before'] as num).toInt(),
    dueDate: j['due_date'] as String,
    mealDate: j['meal_date'] as String,
  );
}

class ShoppingItem {
  final int id;
  final String text;
  final bool done; // derived from 0/1
  final String? category;
  final String notes;
  final List<int> recipeIds;
  final String? recipeTitles; // Comma-separated

  ShoppingItem({
    required this.id,
    required this.text,
    required this.done,
    this.category,
    this.notes = '',
    this.recipeIds = const [],
    this.recipeTitles,
  });

  factory ShoppingItem.fromJson(Map<String, dynamic> j) {
    final doneRaw = j['done'];
    final doneBool = switch (doneRaw) {
      bool b => b,
      num n => n.toInt() != 0,
      String s => int.tryParse(s) != null && int.parse(s) != 0,
      _ => false,
    };

    // Parse recipe_ids JSON array
    List<int> recipeIds = [];
    if (j['recipe_ids'] != null && j['recipe_ids'] is String) {
      try {
        final decoded = jsonDecode(j['recipe_ids'] as String);
        if (decoded is List) {
          recipeIds = decoded.map((e) => (e as num).toInt()).toList();
        }
      } catch (_) {
        // Ignore parse errors
      }
    }

    return ShoppingItem(
      id: (j['id'] as num).toInt(),
      text: j['text'] as String,
      done: doneBool,
      category: j['category'] as String?,
      notes: (j['notes'] as String?) ?? '',
      recipeIds: recipeIds,
      recipeTitles: j['recipe_titles'] as String?,
    );
  }
}

class ShoppingCategory {
  final int id;
  final String name;
  final int sortOrder;
  final String createdAt;

  ShoppingCategory({
    required this.id,
    required this.name,
    required this.sortOrder,
    required this.createdAt,
  });

  factory ShoppingCategory.fromJson(Map<String, dynamic> j) => ShoppingCategory(
    id: (j['id'] as num).toInt(),
    name: j['name'] as String,
    sortOrder: (j['sort_order'] as num).toInt(),
    createdAt: j['created_at'] as String,
  );
}

class Ingredient {
  final double? quantity;
  final String? unit;
  final String name;
  final String? prep;
  /// true = raw unparsed text; false = user-confirmed structured ingredient.
  final bool raw;
  /// Non-null → this item is a section header (not an actual ingredient).
  final String? section;

  bool get isSection => section != null;

  Ingredient({this.quantity, this.unit, required this.name, this.prep, this.raw = false, this.section});

  /// Creates a section-header placeholder (not a real ingredient).
  Ingredient.sectionHeader(String sectionName)
      : quantity = null,
        unit = null,
        name = '',
        prep = null,
        raw = false,
        section = sectionName;

  factory Ingredient.fromJson(Map<String, dynamic> j) {
    // Section header: {"section": "Sauce"}
    final sectionVal = j['section'] as String?;
    if (sectionVal != null && sectionVal.isNotEmpty) {
      return Ingredient.sectionHeader(sectionVal);
    }

    String? prep;

    final p = j['prep'];
    if (p is String && p.trim().isNotEmpty) {
      prep = p.trim();
    } else {
      final pw = j['prep_words'];
      if (pw is List) {
        final words = pw
            .whereType<String>()
            .map((s) => s.trim())
            .where((s) => s.isNotEmpty)
            .toList();
        if (words.isNotEmpty) prep = words.join(' ');
      }
    }

    return Ingredient(
      quantity: (j['quantity'] is num)
          ? (j['quantity'] as num).toDouble()
          : null,
      unit: (j['unit'] as String?)?.isNotEmpty == true
          ? j['unit'] as String
          : null,
      name: j['name'] as String? ?? '',
      prep: prep,
      raw: j['raw'] == true,
    );
  }

  Map<String, dynamic> toJson() {
    if (isSection) return {'section': section};
    return {
      'quantity': quantity,
      'unit': unit,
      'name': name,
      if (prep != null) 'prep': prep,
      'raw': raw,
    };
  }
}

extension IngredientFormat on Ingredient {
  String toLine({double factor = 1.0, bool includePrep = true}) {
    double? q = quantity;
    if (q != null) q = q * factor;

    String trimZeros(String s) => s.replaceFirst(RegExp(r'\.?0+$'), '');

    String numStr(double v, String? u) {
      if (u == 'g' || u == 'ml') {
        return v.round().toString();
      }
      if (u == 'kg' || u == 'L') {
        return trimZeros(v.toStringAsFixed(2));
      }
      final s = ((v * 100).round() / 100.0).toString();
      return trimZeros(s);
    }

    final prepSuffix = (includePrep && prep != null && prep!.isNotEmpty)
        ? ', ${prep!}'
        : '';

    if (q != null && unit != null && unit!.isNotEmpty) {
      return '${numStr(q, unit)} $unit $name$prepSuffix';
    } else if (q != null) {
      final s = ((q * 100).round() / 100.0).toString();
      return '${trimZeros(s)} $name$prepSuffix';
    } else {
      return '$name$prepSuffix';
    }
  }
}

class Recipe {
  final int id;
  final String title;
  final String source;
  final String yieldText;
  final String notes;
  final String createdAt;
  final String updatedAt;
  final List<Ingredient> ingredients;
  final List<String> instructions;
  final String? imagePathSmall;
  final String? imagePathFull;

  // NEW:
  final RecipeMacros? macros;
  final String? shareToken;
  final List<PrepReminder> prepReminders;

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
    this.macros,
    this.shareToken,
    this.prepReminders = const [],
  });

  factory Recipe.fromJson(Map<String, dynamic> j) => Recipe(
    id: j['id'] as int,
    title: j['title'] as String,
    source: j['source'] as String,
    yieldText: j['yield'] as String,
    notes: j['notes'] as String,
    createdAt: j['created_at'] as String,
    updatedAt: j['updated_at'] as String,
    ingredients: (j['ingredients'] as List<dynamic>)
        .map((e) => Ingredient.fromJson(e as Map<String, dynamic>))
        .toList(),
    instructions: (j['instructions'] as List<dynamic>).cast<String>(),
    imagePathSmall: j['image_path_small'] as String?,
    imagePathFull: j['image_path_full'] as String?,
    shareToken: j['share_token'] as String?,
    prepReminders: (() {
      final raw = j['prep_reminders'];
      if (raw is List) {
        return raw
            .map((e) => PrepReminder.fromJson(e as Map<String, dynamic>))
            .toList();
      }
      return <PrepReminder>[];
    })(),
    macros: (() {
      try {
        if (j['macros'] is Map ||
            j['protein'] is num ||
            j['protein_g'] is num) {
          return RecipeMacros.fromAny(j);
        }
      } catch (_) {}
      return null;
    })(),
  );
}

/* =========================
 * Recipes
 * ========================= */
Future<Recipe> estimateRecipeMacros(int id) async {
  final r = await http.post(
    _u('/recipes/$id/macros/estimate'),
    headers: _headers(),
  );
  if (r.statusCode != 200) _throw(r);
  return Recipe.fromJson(jsonDecode(r.body) as Map<String, dynamic>);
}

Future<List<Ingredient>> reparseIngredients(int id) async {
  final r = await http.post(
    _u('/recipes/$id/reparse-ingredients'),
    headers: _headers(),
  );
  if (r.statusCode != 200) _throw(r);
  final List data = jsonDecode(r.body) as List;
  return data
      .map((e) => Ingredient.fromJson(e as Map<String, dynamic>))
      .toList();
}



Future<Recipe> importRecipeFromUrl({required String url, String? model, bool dryRun = false}) async {
  final uri = Uri.parse('$baseUrl/recipes/import');
  final resp = await http.post(
    uri,
    headers: _headers(const {'Content-Type': 'application/json'}),
    body: jsonEncode({
      'url': url,
      if (model != null && model.isNotEmpty) 'model': model,
      if (dryRun) 'dry_run': true,
    }),
  );

  if (resp.statusCode < 200 || resp.statusCode >= 300) {
    String msg = 'Import failed (${resp.statusCode})';
    try {
      final body = jsonDecode(resp.body);
      if (body is Map && body['error'] != null) {
        msg = '$msg: ${body['error']}';
      }
    } catch (_) {}
    throw Exception(msg);
  }

  final json = jsonDecode(resp.body) as Map<String, dynamic>;
  return Recipe.fromJson(json);
}

/// Import a recipe from 1–3 images. Each entry is `(filename, bytes)`.
/// Optionally pass a [model] to override the server's default vision model.
Future<Recipe> importRecipeFromImages(
  List<(String, List<int>)> images, {
  String? model,
}) async {
  final uri = Uri.parse('$baseUrl/recipes/import/images');
  final req = http.MultipartRequest('POST', uri);
  if (_authToken != null && _authToken!.isNotEmpty) {
    req.headers['Authorization'] = 'Bearer $_authToken';
  }
  if (model != null && model.isNotEmpty) {
    req.fields['model'] = model;
  }
  for (final (name, bytes) in images) {
    final ct = lookupMimeType(name) ?? 'image/jpeg';
    req.files.add(
      http.MultipartFile.fromBytes(
        'image',
        bytes,
        filename: name,
        contentType: MediaType.parse(ct),
      ),
    );
  }
  final resp = await http.Response.fromStream(await req.send());
  if (resp.statusCode != 200) _throw(resp);
  return Recipe.fromJson(jsonDecode(resp.body) as Map<String, dynamic>);
}

/// Parses a single ingredient text line (e.g. "200 g flour") into a
/// structured [Ingredient]. Recognises the canonical units stored by the
/// backend (g, kg, ml, L, tsp, tbsp); everything else is treated as part of
/// Parses a single ingredient line into a structured [Ingredient] proposal.
///
/// The returned ingredient always has `raw: false` — it is a proposed parse,
/// not a raw unprocessed line.
Ingredient parseIngredientLine(String text) {
  final tokens = text.trim().split(RegExp(r'\s+'));
  if (tokens.isEmpty) {
    return Ingredient(name: text);
  }

  final qty = double.tryParse(tokens[0].replaceAll(',', '.'));
  if (qty == null) {
    // No leading number — the whole text is the name.
    return Ingredient(name: text);
  }
  if (tokens.length < 2) {
    return Ingredient(name: text);
  }

  // Only the units the backend stores as canonical.
  const knownUnits = {'g', 'kg', 'ml', 'L', 'tsp', 'tbsp'};

  int nameIdx = 1;
  String? unit;
  if (knownUnits.contains(tokens[1])) {
    unit = tokens[1];
    nameIdx = 2;
    if (nameIdx < tokens.length && tokens[nameIdx].toLowerCase() == 'of') {
      nameIdx++;
    }
  }

  if (nameIdx >= tokens.length) {
    return Ingredient(name: text);
  }

  return Ingredient(
    name: tokens.sublist(nameIdx).join(' '),
    quantity: qty,
    unit: unit,
  );
}

Future<Recipe> updateRecipe({
  required int id,
  String? title,
  String? source,
  String? yieldText,
  String? notes,
  List<Ingredient>? ingredients,
  List<String>? instructions,
}) async {
  final body = <String, dynamic>{
    if (title != null) 'title': title,
    if (source != null) 'source': source,
    if (yieldText != null) 'yield': yieldText,
    if (notes != null) 'notes': notes,
    if (ingredients != null) 'ingredients': ingredients.map((i) => i.toJson()).toList(),
    if (instructions != null) 'instructions': instructions,
  };
  final r = await http.patch(
    _u('/recipes/$id'),
    headers: _headers({'content-type': 'application/json'}),
    body: jsonEncode(body),
  );
  if (r.statusCode != 200) _throw(r);
  return Recipe.fromJson(jsonDecode(r.body) as Map<String, dynamic>);
}

Future<Recipe> updateRecipePrepReminders({
  required int id,
  required List<PrepReminder> prepReminders,
}) async {
  final r = await http.patch(
    _u('/recipes/$id'),
    headers: _headers({'content-type': 'application/json'}),
    body: jsonEncode({'prep_reminders': prepReminders.map((r) => r.toJson()).toList()}),
  );
  if (r.statusCode != 200) _throw(r);
  return Recipe.fromJson(jsonDecode(r.body) as Map<String, dynamic>);
}

Future<Recipe> uploadRecipeImage({
  required int id,
  String? path,
  List<int>? bytes,
  String? filename,
}) async {
  final url = _u('/recipes/$id/image');
  final req = http.MultipartRequest('POST', url);

  if (_authToken != null && _authToken!.isNotEmpty) {
    req.headers['Authorization'] = 'Bearer $_authToken';
  }

  if (bytes != null) {
    final name =
        filename ??
        (path != null && path.isNotEmpty ? p.basename(path) : 'upload');
    final ct = lookupMimeType(name) ?? 'application/octet-stream';
    req.files.add(
      http.MultipartFile.fromBytes(
        'image',
        bytes,
        filename: name,
        contentType: MediaType.parse(ct),
      ),
    );
  } else if (path != null && path.isNotEmpty) {
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
  } else {
    throw Exception('Provide bytes or a file path');
  }

  final resp = await http.Response.fromStream(await req.send());
  if (resp.statusCode != 200) _throw(resp);
  return Recipe.fromJson(jsonDecode(resp.body) as Map<String, dynamic>);
}

Future<List<Recipe>> fetchRecipes() async {
  final res = await http.get(_u('/recipes'), headers: _headers(null, false));
  if (res.statusCode != 200) {
    throw Exception('HTTP ${res.statusCode}: ${res.body}');
  }
  final List data = jsonDecode(res.body) as List;
  return data.map((e) => Recipe.fromJson(e as Map<String, dynamic>)).toList();
}

Future<Recipe> fetchRecipe(int id) async {
  final res = await http.get(_u('/recipes/$id'), headers: _headers(null, false));
  if (res.statusCode != 200) {
    throw Exception('HTTP ${res.statusCode}: ${res.body}');
  }
  return Recipe.fromJson(jsonDecode(res.body) as Map<String, dynamic>);
}

Future<Recipe> createRecipeFull({
  required String title,
  String? source,
  String? yieldText,
  String? notes,
  required List<Ingredient> ingredients,
  required List<String> instructions,
}) async {
  final uri = Uri.parse('$baseUrl/recipes');

  final body = <String, dynamic>{
    'title': title,
    'source': source,
    'yield': yieldText,
    'notes': notes,
    'ingredients': ingredients.map((i) => i.toJson()).toList(),
    'instructions': instructions,
  };

  final res = await http.post(
    uri,
    headers: _headers(const {
      'Content-Type': 'application/json; charset=utf-8',
      'Accept': 'application/json',
    }),
    body: jsonEncode(body),
  );

  if (res.statusCode != 200) {
    throw Exception('createRecipeFull: ${res.statusCode} ${res.body}');
  }
  return Recipe.fromJson(jsonDecode(res.body) as Map<String, dynamic>);
}

Future<void> deleteRecipe(int id) async {
  final res = await http.delete(_u('/recipes/$id'), headers: _headers());
  if (res.statusCode != 200 && res.statusCode != 204) {
    throw Exception('HTTP ${res.statusCode}: ${res.body}');
  }
}

/// Fetches soft-deleted recipes (trash).
Future<List<Recipe>> fetchDeletedRecipes() async {
  final res = await http.get(_u('/recipes/deleted'), headers: _headers());
  if (res.statusCode != 200) {
    throw Exception('HTTP ${res.statusCode}: ${res.body}');
  }
  final List<dynamic> data = jsonDecode(res.body) as List<dynamic>;
  return data
      .map((e) => Recipe.fromJson(e as Map<String, dynamic>))
      .toList();
}

/// Restores a soft-deleted recipe.
Future<Recipe> restoreRecipe(int id) async {
  final res = await http.post(_u('/recipes/$id/restore'), headers: _headers());
  if (res.statusCode != 200) {
    throw Exception('HTTP ${res.statusCode}: ${res.body}');
  }
  return Recipe.fromJson(jsonDecode(res.body) as Map<String, dynamic>);
}

/// Permanently deletes a recipe from trash.
Future<void> permanentDeleteRecipe(int id) async {
  final res = await http.delete(_u('/recipes/$id/permanent'), headers: _headers());
  if (res.statusCode != 200 && res.statusCode != 204) {
    throw Exception('HTTP ${res.statusCode}: ${res.body}');
  }
}

/// Duplicate match returned by checkDuplicate
class DuplicateMatch {
  final int id;
  final String title;
  final String source;
  final String matchType; // "url" or "title"

  DuplicateMatch({
    required this.id,
    required this.title,
    required this.source,
    required this.matchType,
  });

  factory DuplicateMatch.fromJson(Map<String, dynamic> json) {
    return DuplicateMatch(
      id: (json['id'] as num).toInt(),
      title: json['title'] as String,
      source: json['source'] as String? ?? '',
      matchType: json['match_type'] as String,
    );
  }
}

/// Check if a recipe with the same URL or similar title already exists.
Future<List<DuplicateMatch>> checkDuplicate({String? url, String? title}) async {
  final res = await http.post(
    _u('/recipes/check-duplicate'),
    headers: _headers({'content-type': 'application/json'}),
    body: jsonEncode({'url': url, 'title': title}),
  );
  if (res.statusCode != 200) {
    throw Exception('HTTP ${res.statusCode}: ${res.body}');
  }
  final data = jsonDecode(res.body) as Map<String, dynamic>;
  final List<dynamic> duplicates = data['duplicates'] as List<dynamic>;
  return duplicates
      .map((e) => DuplicateMatch.fromJson(e as Map<String, dynamic>))
      .toList();
}

/// Fetches a shared recipe by token (no auth required).
Future<Recipe> fetchSharedRecipe(String token) async {
  final res = await http.get(_u('/api/share/$token'));
  if (res.statusCode == 404) throw Exception('Share link not found or expired');
  if (res.statusCode != 200) throw Exception('HTTP ${res.statusCode}: ${res.body}');
  return Recipe.fromJson(jsonDecode(res.body) as Map<String, dynamic>);
}

/// Returns the share token (creates one if not yet set).
Future<String> shareRecipe(int id) async {
  final res = await http.post(_u('/recipes/$id/share'), headers: _headers());
  if (res.statusCode != 200) throw Exception('HTTP ${res.statusCode}: ${res.body}');
  final data = jsonDecode(res.body) as Map<String, dynamic>;
  return data['share_token'] as String;
}

/// Revokes the share token.
Future<void> revokeRecipeShare(int id) async {
  final res = await http.delete(_u('/recipes/$id/share'), headers: _headers());
  if (res.statusCode != 204) throw Exception('HTTP ${res.statusCode}: ${res.body}');
}

/* =========================
 * Meal plan & Shopping (unchanged)
 * ========================= */

Future<List<MealPlanEntry>> fetchMealPlanForDay(String day) async {
  final r = await http.get(_u('/meal-plan', {'day': day}), headers: _headers());
  if (r.statusCode != 200) _throw(r);
  final List data = jsonDecode(r.body) as List;
  return data
      .map((e) => MealPlanEntry.fromJson(e as Map<String, dynamic>))
      .toList();
}

Future<List<MealPlanEntry>> fetchMealPlanForRecipe(int recipeId) async {
  final r = await http.get(
    _u('/meal-plan/recipe/$recipeId'),
    headers: _headers(),
  );
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
    headers: _headers({'content-type': 'application/json'}),
    body: jsonEncode({'day': day, 'recipe_id': recipeId}),
  );
  if (r.statusCode != 200) _throw(r);
  return MealPlanEntry.fromJson(jsonDecode(r.body) as Map<String, dynamic>);
}

Future<void> unassignRecipeFromDay({
  required String day,
  required int recipeId,
}) async {
  final r = await http.delete(
    _u('/meal-plan/$day/$recipeId'),
    headers: _headers(),
  );
  if (r.statusCode != 200) _throw(r);
}

Future<MealPlanEntry> moveMealPlanEntry({
  required String fromDay,
  required String toDay,
  required int recipeId,
}) async {
  final r = await http.patch(
    _u('/meal-plan/$fromDay/$recipeId'),
    headers: {..._headers(), 'Content-Type': 'application/json'},
    body: jsonEncode({'new_day': toDay}),
  );
  if (r.statusCode != 200) _throw(r);
  return MealPlanEntry.fromJson(jsonDecode(r.body) as Map<String, dynamic>);
}

Future<List<PrepReminderDto>> fetchUpcomingReminders() async {
  final now = DateTime.now();
  final from = '${now.year.toString().padLeft(4, '0')}-'
      '${now.month.toString().padLeft(2, '0')}-'
      '${now.day.toString().padLeft(2, '0')}';
  final to30 = now.add(const Duration(days: 30));
  final to = '${to30.year.toString().padLeft(4, '0')}-'
      '${to30.month.toString().padLeft(2, '0')}-'
      '${to30.day.toString().padLeft(2, '0')}';
  final r = await http.get(
    _u('/meal-plan/reminders', {'from': from, 'to': to}),
    headers: _headers(),
  );
  if (r.statusCode != 200) _throw(r);
  final List data = jsonDecode(r.body) as List;
  return data
      .map((e) => PrepReminderDto.fromJson(e as Map<String, dynamic>))
      .toList();
}

Future<List<ShoppingItem>> fetchShoppingList() async {
  final r = await http.get(_u('/shopping'), headers: _headers());
  if (r.statusCode != 200) _throw(r);
  final List data = jsonDecode(r.body) as List;
  return data
      .map((e) => ShoppingItem.fromJson(e as Map<String, dynamic>))
      .toList();
}

Future<List<String>> fetchAllShoppingTexts() async {
  final r = await http.get(_u('/shopping/all-texts'), headers: _headers());
  if (r.statusCode != 200) _throw(r);
  final List data = jsonDecode(r.body) as List;
  return data.cast<String>();
}

Future<List<ShoppingItem>> mergeShoppingIngredients(
  List<Ingredient> items, {
  int? recipeId,
}) async {
  final uri = _u('/shopping/merge');
  final body = {
    'items': items.map((e) => e.toJson()).toList(),
    if (recipeId != null) 'recipe_id': recipeId,
  };
  final r = await http.post(
    uri,
    headers: _headers({'content-type': 'application/json'}),
    body: jsonEncode(body),
  );
  if (r.statusCode != 200) _throw(r);
  final List data = jsonDecode(r.body) as List;
  return data
      .map((e) => ShoppingItem.fromJson(e as Map<String, dynamic>))
      .toList();
}

Future<ShoppingItem> createShoppingItem(String text) async {
  final r = await http.post(
    _u('/shopping'),
    headers: _headers({'content-type': 'application/json'}),
    body: jsonEncode({'text': text, 'raw': true}),
  );
  if (r.statusCode != 200) _throw(r);
  return ShoppingItem.fromJson(jsonDecode(r.body) as Map<String, dynamic>);
}

Future<ShoppingItem> toggleShoppingItem({
  required int id,
  required bool done,
}) => updateShoppingItem(id: id, done: done);

Future<void> deleteShoppingItem(int id) async {
  final r = await http.delete(_u('/shopping/$id'), headers: _headers());
  if (r.statusCode != 200) _throw(r);
}

// ── Shopping Categories ───────────────────────────────────────────────────────

Future<List<ShoppingCategory>> fetchCategories() async {
  final r = await http.get(_u('/categories'), headers: _headers());
  if (r.statusCode != 200) _throw(r);
  final List data = jsonDecode(r.body) as List;
  return data
      .map((e) => ShoppingCategory.fromJson(e as Map<String, dynamic>))
      .toList();
}

Future<ShoppingCategory> createCategory(String name) async {
  final r = await http.post(
    _u('/categories'),
    headers: _headers({'content-type': 'application/json'}),
    body: jsonEncode({'name': name}),
  );
  if (r.statusCode != 200) _throw(r);
  return ShoppingCategory.fromJson(jsonDecode(r.body) as Map<String, dynamic>);
}

Future<ShoppingCategory> updateCategory(int id, {String? name, int? sortOrder}) async {
  final body = <String, dynamic>{
    if (name != null) 'name': name,
    if (sortOrder != null) 'sort_order': sortOrder,
  };
  final r = await http.patch(
    _u('/categories/$id'),
    headers: _headers({'content-type': 'application/json'}),
    body: jsonEncode(body),
  );
  if (r.statusCode != 200) _throw(r);
  return ShoppingCategory.fromJson(jsonDecode(r.body) as Map<String, dynamic>);
}

Future<void> deleteCategory(int id) async {
  final r = await http.delete(_u('/categories/$id'), headers: _headers());
  if (r.statusCode != 200 && r.statusCode != 204) _throw(r);
}

Future<List<ShoppingCategory>> reorderCategories(List<int> orderedIds) async {
  final r = await http.post(
    _u('/categories/reorder'),
    headers: _headers({'content-type': 'application/json'}),
    body: jsonEncode({'order': orderedIds}),
  );
  if (r.statusCode != 200) _throw(r);
  final List data = jsonDecode(r.body) as List;
  return data
      .map((e) => ShoppingCategory.fromJson(e as Map<String, dynamic>))
      .toList();
}

// ── LLM Credits ───────────────────────────────────────────────────────────────

class LlmCredits {
  final double usage;
  final double? limit;
  final bool isFreeTier;

  const LlmCredits({
    required this.usage,
    this.limit,
    required this.isFreeTier,
  });

  factory LlmCredits.fromJson(Map<String, dynamic> j) => LlmCredits(
    usage: (j['usage'] as num?)?.toDouble() ?? 0.0,
    limit: (j['limit'] as num?)?.toDouble(),
    isFreeTier: j['is_free_tier'] as bool? ?? false,
  );
}

Future<LlmCredits> fetchLlmCredits() async {
  final r = await http.get(_u('/llm/credits'), headers: _headers());
  if (r.statusCode != 200) _throw(r);
  return LlmCredits.fromJson(jsonDecode(r.body) as Map<String, dynamic>);
}

