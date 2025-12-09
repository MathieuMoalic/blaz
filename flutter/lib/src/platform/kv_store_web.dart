import 'package:web/web.dart' as web;

Future<String?> getString(String key) async => web.window.localStorage[key];

Future<void> setString(String key, String value) async {
  web.window.localStorage[key] = value;
}
