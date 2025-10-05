import 'package:flutter/foundation.dart'
    show kIsWeb, defaultTargetPlatform, TargetPlatform;
import 'package:flutter/material.dart';
import 'package:window_manager/window_manager.dart';

import 'package:blaz/src/views/recipes_page.dart';
import 'package:blaz/src/views/add_recipe_page.dart';
import 'package:blaz/src/views/meal_plan_page.dart';
import 'package:blaz/src/views/shopping_list_page.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();

  final isDesktop =
      !kIsWeb &&
      (defaultTargetPlatform == TargetPlatform.linux ||
          defaultTargetPlatform == TargetPlatform.windows ||
          defaultTargetPlatform == TargetPlatform.macOS);

  if (isDesktop) {
    await windowManager.ensureInitialized();
    const windowOptions = WindowOptions(
      titleBarStyle: TitleBarStyle.hidden,
      windowButtonVisibility: false,
      backgroundColor: Colors.transparent,
    );
    windowManager.waitUntilReadyToShow(windowOptions, () async {
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
      home: const _HomeShell(),
      debugShowCheckedModeBanner: false,
    );
  }
}

class _HomeShell extends StatefulWidget {
  const _HomeShell();
  @override
  State<_HomeShell> createState() => _HomeShellState();
}

class _HomeShellState extends State<_HomeShell> {
  int _index = 0;
  final _recipesKey = GlobalKey<RecipesPageState>();

  @override
  Widget build(BuildContext context) {
    final isDesktop =
        !kIsWeb &&
        (defaultTargetPlatform == TargetPlatform.linux ||
            defaultTargetPlatform == TargetPlatform.windows ||
            defaultTargetPlatform == TargetPlatform.macOS);

    final pages = <Widget>[
      RecipesPage(key: _recipesKey),
      const MealPlanPage(),
      const ShoppingListPage(),
    ];

    return Scaffold(
      body: Column(
        children: [
          if (isDesktop) ...[
            GestureDetector(
              behavior: HitTestBehavior.opaque,
              onPanStart: (_) => windowManager.startDragging(),
              child: Container(
                height: 44,
                padding: const EdgeInsets.symmetric(horizontal: 12),
                alignment: Alignment.centerLeft,
                child: Text(
                  'Blaz',
                  style: Theme.of(context).textTheme.titleMedium,
                ),
              ),
            ),
            const Divider(height: 1),
          ],
          Expanded(
            child: IndexedStack(index: _index, children: pages),
          ),
        ],
      ),
      bottomNavigationBar: NavigationBar(
        selectedIndex: _index,
        onDestinationSelected: (i) => setState(() => _index = i),
        destinations: const [
          NavigationDestination(
            icon: Icon(Icons.menu_book_outlined),
            selectedIcon: Icon(Icons.menu_book),
            label: 'Recipes',
          ),
          NavigationDestination(
            icon: Icon(Icons.calendar_today_outlined),
            selectedIcon: Icon(Icons.calendar_month),
            label: 'Meal plan',
          ),
          NavigationDestination(
            icon: Icon(Icons.shopping_cart_outlined),
            selectedIcon: Icon(Icons.shopping_cart),
            label: 'Shopping',
          ),
        ],
      ),
      floatingActionButton: _index == 0
          ? FloatingActionButton.extended(
              icon: const Icon(Icons.add),
              label: const Text('Add'),
              onPressed: () async {
                final created = await Navigator.of(context).push<bool>(
                  MaterialPageRoute(builder: (_) => const AddRecipePage()),
                );
                if (created == true) _recipesKey.currentState?.refresh();
              },
            )
          : null,
    );
  }
}
