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

const kBgAvif = 'assets/images/background.avif';
const kBgFallback = 'assets/images/background.png';

class BlazApp extends StatelessWidget {
  const BlazApp({super.key});

  ThemeData _theme(Brightness b) => ThemeData(
    useMaterial3: true,
    colorScheme: ColorScheme.fromSeed(seedColor: Colors.teal, brightness: b),
  );

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Blaz',
      themeMode: ThemeMode.dark,
      theme: _theme(
        Brightness.light,
      ).copyWith(scaffoldBackgroundColor: Colors.transparent),
      darkTheme: _theme(
        Brightness.dark,
      ).copyWith(scaffoldBackgroundColor: Colors.transparent),
      builder: (context, child) {
        final tint = Theme.of(context).brightness == Brightness.dark
            ? Colors.black.withAlpha(80)
            : Colors.white.withAlpha(40);

        Widget bg = Image.asset(
          kBgAvif,
          fit: BoxFit.cover,
          errorBuilder: (_, __, ___) {
            // Fallback if AVIF isn't supported or not bundled
            return Image.asset(kBgFallback, fit: BoxFit.cover);
          },
        );

        return Stack(
          children: [
            Positioned.fill(child: bg),
            Positioned.fill(
              child: IgnorePointer(child: ColoredBox(color: tint)),
            ),
            if (child != null) child,
          ],
        );
      },

      home: Auth.token == null ? const LoginPage() : const HomeShell(),
      debugShowCheckedModeBanner: false,
    );
  }
}
