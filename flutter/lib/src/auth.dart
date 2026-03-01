import './api.dart' as api;
import './platform/kv_store.dart' as kv;

class Auth {
  static String? _token;

  static Future<void> init() async {
    _token = await kv.getString('auth_token');
    api.setAuthToken(_token);
  }

  static String? get token => _token;

  static Future<void> save(String token) async {
    _token = token;
    await kv.setString('auth_token', token);
    api.setAuthToken(token);
  }

  static Future<void> logout() async {
    _token = null;
    final prefs = await kv.getString('auth_token');
    if (prefs != null) {
      await kv.setString('auth_token', '');
    }
    api.setAuthToken(null);
  }

  static Future<void> login({required String password}) async {
    final token = await api.login(password: password);
    await save(token);
  }
}
