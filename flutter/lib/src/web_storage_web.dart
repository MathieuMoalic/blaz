import 'dart:html' as html;

String? read(String key) {
  try {
    return html.window.localStorage[key];
  } catch (_) {
    return null;
  }
}

void write(String key, String? value) {
  try {
    final s = html.window.localStorage;
    if (value == null || value.isEmpty) {
      s.remove(key);
    } else {
      s[key] = value;
    }
  } catch (_) {}
}
