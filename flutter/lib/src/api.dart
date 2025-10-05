import 'dart:convert';
import 'package:http/http.dart' as http;

class Recipe {
  final int id;
  final String title;
  Recipe({required this.id, required this.title});

  factory Recipe.fromJson(Map<String, dynamic> j) =>
      Recipe(id: j['id'] as int, title: j['title'] as String);
}

// Compile-time base URL; override with --dart-define
const String _baseUrl = String.fromEnvironment(
  'API_BASE_URL',
  // For Android with `adb reverse`, prefer 127.0.0.1; for LAN, set via --dart-define
  defaultValue: 'http://127.0.0.1:8080',
);

final http.Client _client = http.Client();
const _timeout = Duration(seconds: 10);

Future<List<Recipe>> fetchRecipes() async {
  final uri = Uri.parse('$_baseUrl/recipes');
  final res = await _client.get(uri).timeout(_timeout);
  if (res.statusCode != 200) {
    throw Exception('GET /recipes → HTTP ${res.statusCode}: ${res.body}');
  }
  final List data = jsonDecode(res.body) as List;
  return data.map((e) => Recipe.fromJson(e as Map<String, dynamic>)).toList();
}

Future<Recipe> createRecipe(String title) async {
  final uri = Uri.parse('$_baseUrl/recipes');
  final res = await _client
      .post(uri,
          headers: {'Content-Type': 'application/json'},
          body: jsonEncode({'title': title}))
      .timeout(_timeout);

  if (res.statusCode < 200 || res.statusCode >= 300) {
    throw Exception('POST /recipes → HTTP ${res.statusCode}: ${res.body}');
  }
  return Recipe.fromJson(jsonDecode(res.body) as Map<String, dynamic>);
}

