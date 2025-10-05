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
  defaultValue: 'http://192.168.1.81:8080',
);

Future<List<Recipe>> fetchRecipes() async {
  final uri = Uri.parse('$_baseUrl/recipes');
  final res = await http.get(uri);
  if (res.statusCode != 200) {
    throw Exception('HTTP ${res.statusCode}: ${res.body}');
  }
  final List data = jsonDecode(res.body) as List;
  return data.map((e) => Recipe.fromJson(e as Map<String, dynamic>)).toList();
}

