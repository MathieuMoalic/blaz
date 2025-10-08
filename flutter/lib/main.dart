import 'dart:io' show Platform;
import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:flutter/material.dart';
import 'package:window_manager/window_manager.dart';

import 'src/views/recipes_page.dart';
import 'src/views/add_recipe_page.dart';
import 'src/views/meal_plan/meal_plan_page.dart';
import 'src/views/shopping_list_page.dart';

void main() async {
  WidgetsFlutterBinding.ensureInitialized();

  // optional: frameless desktop window like you had
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
      home: const HomeShell(),
      debugShowCheckedModeBanner: false,
    );
  }
}

class HomeShell extends StatefulWidget {
  const HomeShell({super.key});
  @override
  State<HomeShell> createState() => _HomeShellState();
}

class _HomeShellState extends State<HomeShell> {
  int _index = 0;
  final _recipesKey = GlobalKey<RecipesPageState>();

  @override
  Widget build(BuildContext context) {
    final pages = [
      RecipesPage(key: _recipesKey),
      const MealPlanPage(),
      const ShoppingListPage(),
    ];

    return Scaffold(
      body: SafeArea(child: pages[_index]),
      bottomNavigationBar: NavigationBar(
        selectedIndex: _index,
        destinations: const [
          NavigationDestination(
            icon: Icon(Icons.restaurant_menu),
            label: 'Recipes',
          ),
          NavigationDestination(
            icon: Icon(Icons.calendar_month),
            label: 'Meal plan',
          ),
          NavigationDestination(
            icon: Icon(Icons.shopping_cart),
            label: 'Shopping',
          ),
        ],
        onDestinationSelected: (i) => setState(() => _index = i),
      ),
      floatingActionButton: _index == 0
          ? FloatingActionButton.extended(
              onPressed: () async {
                final created = await Navigator.of(context).push<bool>(
                  MaterialPageRoute(builder: (_) => const AddRecipePage()),
                );
                if (created == true) {
                  _recipesKey.currentState?.refresh();
                }
              },
              icon: const Icon(Icons.add),
              label: const Text('Recipe'),
            )
          : null,
    );
  }
}
