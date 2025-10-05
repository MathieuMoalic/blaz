import 'dart:io' show Platform;
import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:flutter/material.dart';
import 'package:window_manager/window_manager.dart';
import 'src/api.dart';

void main() async {
  WidgetsFlutterBinding.ensureInitialized();

  // Desktop-only: remove native title bar & buttons
  if (!kIsWeb && (Platform.isWindows || Platform.isLinux || Platform.isMacOS)) {
    await windowManager.ensureInitialized();

    final windowOptions = const WindowOptions(
      titleBarStyle: TitleBarStyle.hidden, // macOS-style hidden title bar
      windowButtonVisibility: false,       // hide the traffic lights on macOS
      backgroundColor: Colors.transparent, // prettier edges for dark UIs
    );

    windowManager.waitUntilReadyToShow(windowOptions, () async {
      await windowManager.setAsFrameless(); // removes frame on Win/Linux/macOS
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
    final light = ColorScheme.fromSeed(seedColor: Colors.teal, brightness: Brightness.light);
    final dark  = ColorScheme.fromSeed(seedColor: Colors.teal, brightness: Brightness.dark);

    return MaterialApp(
      title: 'Blaz',
      themeMode: ThemeMode.dark,                  // always dark; change to system if you want
      theme: ThemeData(useMaterial3: true, colorScheme: light),
      darkTheme: ThemeData(useMaterial3: true, colorScheme: dark),
      home: const RecipesPage(),
      debugShowCheckedModeBanner: false,
    );
  }
}

class RecipesPage extends StatefulWidget {
  const RecipesPage({super.key});
  @override
  State<RecipesPage> createState() => _RecipesPageState();
}

class _RecipesPageState extends State<RecipesPage> {
  late Future<List<Recipe>> _future;

  @override
  void initState() {
    super.initState();
    _future = fetchRecipes();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      // We removed the native title bar, so make a draggable Flutter one:
      body: Column(
        children: [
          // Drag anywhere in this area to move the window
          DragToMoveArea(
            child: Container(
              height: 44,
              padding: const EdgeInsets.symmetric(horizontal: 12),
              alignment: Alignment.centerLeft,
              child: Text('Blaz', style: Theme.of(context).textTheme.titleMedium),
            ),
          ),
          const Divider(height: 1),
          Expanded(
            child: FutureBuilder<List<Recipe>>(
              future: _future,
              builder: (context, snap) {
                if (snap.connectionState != ConnectionState.done) {
                  return const Center(child: CircularProgressIndicator());
                }
                if (snap.hasError) {
                  return Center(child: Text('Error: ${snap.error}'));
                }
                final items = snap.data ?? const [];
                if (items.isEmpty) {
                  return const Center(child: Text('No recipes yet'));
                }
                return ListView.separated(
                  padding: const EdgeInsets.all(12),
                  itemCount: items.length,
                  itemBuilder: (_, i) => ListTile(title: Text(items[i].title)),
                  separatorBuilder: (_, __) => const Divider(height: 1),
                );
              },
            ),
          ),
        ],
      ),
    );
  }
}

