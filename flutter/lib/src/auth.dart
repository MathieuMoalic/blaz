import 'dart:convert';
import 'package:http/http.dart' as http;

import './api.dart' as api;

// Conditional storage: real localStorage on web, no-op elsewhere
import './web_storage_stub.dart'
    if (dart.library.html) './web_storage_web.dart'
    as webstore;

Future<bool> serverAllowsRegistration() async {
  final uri = Uri.parse('${api.baseUrl}/auth/status');
  final res = await http.get(uri);
  if (res.statusCode != 200) {
    throw Exception('HTTP ${res.statusCode} $uri: ${res.body}');
  }
  final data = jsonDecode(res.body) as Map<String, dynamic>;
  return data['allow_registration'] == true;
}

class Auth {
  static String? _token;
  static bool allowRegistration = true;

  static Future<void> init() async {
    _token = webstore.read('auth_token');
    api.setAuthToken(_token);
    try {
      allowRegistration = await serverAllowsRegistration();
    } catch (_) {
      allowRegistration = true;
    }
  }

  static String? get token => _token;

  static Map<String, String> authHeaders([Map<String, String>? extra]) {
    final h = <String, String>{
      'Content-Type': 'application/json',
      if (_token != null) 'Authorization': 'Bearer $_token',
    };
    if (extra != null) h.addAll(extra);
    return h;
  }

  static Future<void> save(String token) async {
    _token = token;
    webstore.write('auth_token', token);
    api.setAuthToken(token);
  }

  static Future<void> logout() async {
    _token = null;
    webstore.write('auth_token', null);
    api.setAuthToken(null);
  }

  static Future<bool> register({
    required String email,
    required String password,
  }) async {
    final uri = Uri.parse('${api.baseUrl}/auth/register');
    final res = await http.post(
      uri,
      headers: {'Content-Type': 'application/json'},
      body: jsonEncode({'email': email, 'password': password}),
    );

    if (res.statusCode == 201) {
      allowRegistration = false;
      return true;
    }
    if (res.statusCode == 403) throw Exception('Registration is disabled.');
    if (res.statusCode == 409) throw Exception('Email already exists.');
    throw Exception('HTTP ${res.statusCode} $uri: ${res.body}');
  }

  static Future<void> login({
    required String email,
    required String password,
  }) async {
    final token = await api.login(email: email, password: password);
    await save(token);
  }
}
