import 'package:web/web.dart' as web;

String? read(String key) {
  try {
    return web.window.localStorage.getItem(key);
  } catch (_) {
    return null;
  }
}

void write(String key, String? value) {
  try {
    final s = web.window.localStorage;
    if (value == null || value.isEmpty) {
      s.removeItem(key);
    } else {
      s.setItem(key, value);
    }
  } catch (_) {}
}
