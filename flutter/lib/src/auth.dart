import 'dart:convert';
import 'package:http/http.dart' as http;
import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:web/web.dart' as web;

import './api.dart' as api;

class Auth {
  static String? _token;
  static bool allowRegistration = true;

  static Future<void> init() async {
    _token = _readToken();
    try {
      allowRegistration = await api.serverAllowsRegistration();
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
    _writeToken(token);
  }

  static Future<void> logout() async {
    _token = null;
    _writeToken('');
  }

  // --- storage (web localStorage) ---
  static const _storageKey = 'auth_token';

  static String? _readToken() {
    if (!kIsWeb) return null;
    try {
      final storage = web.window.localStorage;
      return storage.getItem(_storageKey);
    } catch (_) {
      return null;
    }
  }

  static void _writeToken(String? value) {
    if (!kIsWeb) return;
    try {
      final storage = web.window.localStorage;
      if (value == null || value.isEmpty) {
        storage.removeItem(_storageKey);
      } else {
        storage.setItem(_storageKey, value);
      }
    } catch (_) {}
  }

  // --- HTTP helpers ---
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
    if (res.statusCode == 201) return true;
    if (res.statusCode == 403) throw Exception('Registration is disabled.');
    if (res.statusCode == 409) throw Exception('Email already exists.');
    throw Exception('HTTP ${res.statusCode} $uri: ${res.body}');
  }

  static Future<void> login({
    required String email,
    required String password,
  }) async {
    final uri = Uri.parse('${api.baseUrl}/auth/login');
    final res = await http.post(
      uri,
      headers: {'Content-Type': 'application/json'},
      body: jsonEncode({'email': email, 'password': password}),
    );
    if (res.statusCode != 200) {
      throw Exception('HTTP ${res.statusCode} $uri: ${res.body}');
    }
    final data = jsonDecode(res.body) as Map<String, dynamic>;
    final token = data['token'] as String?;
    if (token == null || token.isEmpty) {
      throw Exception('Missing token in response.');
    }
    await save(token);
  }
}
