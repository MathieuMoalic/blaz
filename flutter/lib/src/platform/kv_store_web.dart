import 'dart:html' as html;

Future<String?> getString(String key) async => html.window.localStorage[key];

Future<void> setString(String key, String value) async {
  html.window.localStorage[key] = value;
}
