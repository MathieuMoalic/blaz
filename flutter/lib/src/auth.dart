import 'package:shared_preferences/shared_preferences.dart';
import 'api.dart' as api;

/// Minimal auth helper with token persistence.
/// - Auth.init() loads a saved token on startup
/// - Auth.save(token) persists + applies the token
/// - Auth.login/register call your backend and wire the token into api.dart
class Auth {
  static const _kTokenKey = 'auth_token';
  static String? _token;

  static String? get token => _token;
  static bool get isLoggedIn => _token != null && _token!.isNotEmpty;

  /// Load token from storage and attach it to API calls.
  static Future<void> init() async {
    final prefs = await SharedPreferences.getInstance();
    _token = prefs.getString(_kTokenKey);
    api.setAuthToken(_token);
  }

  /// Persist token and attach it to API calls.
  static Future<void> save(String token) async {
    _token = token;
    api.setAuthToken(token);
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString(_kTokenKey, token);
  }

  /// Remove token everywhere (logout).
  static Future<void> clear() async {
    _token = null;
    api.setAuthToken(null);
    final prefs = await SharedPreferences.getInstance();
    await prefs.remove(_kTokenKey);
  }

  static Future<void> register(String email, String password) async {
    await api.register(email: email, password: password);
  }

  static Future<void> login(String email, String password) async {
    final token = await api.login(email: email, password: password);
    await save(token);
  }

  static Future<void> logout() async {
    await clear();
  }
}
