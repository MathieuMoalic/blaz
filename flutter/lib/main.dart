import 'dart:io' show Platform;
import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:flutter/material.dart';
import 'package:window_manager/window_manager.dart';

import 'src/views/login_page.dart';
import 'src/auth.dart';
import 'src/home_shell.dart';

void main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await Auth.init();

  if (!kIsWeb && (Platform.isWindows || Platform.isLinux || Platform.isMacOS)) {
    await windowManager.ensureInitialized();
    const opts = WindowOptions(
      titleBarStyle: TitleBarStyle.hidden,
      windowButtonVisibility: false,
      backgroundColor: Colors.transparent,
    );
    windowManager.waitUntilReadyToShow(opts, () async {
      await windowManager.setAsFrameless();
      await windowManager.show();
      await windowManager.focus();
    });
  }

  runApp(const BlazApp());
}

class BlazApp extends StatelessWidget {
  const BlazApp({super.key});
  @override
  Widget build(BuildContext context) {
    final light = ColorScheme.fromSeed(
      seedColor: Colors.teal,
      brightness: Brightness.light,
    );
    final dark = ColorScheme.fromSeed(
      seedColor: Colors.teal,
      brightness: Brightness.dark,
    );

    return MaterialApp(
      title: 'Blaz',
      themeMode: ThemeMode.dark,
      theme: ThemeData(useMaterial3: true, colorScheme: light),
      darkTheme: ThemeData(useMaterial3: true, colorScheme: dark),
      home: Auth.token == null ? const LoginPage() : const HomeShell(),
      debugShowCheckedModeBanner: false,
    );
  }
}
