import 'package:flutter/material.dart';
import 'views/recipes_page.dart';
import 'views/add_recipe_page.dart';
import 'views/meal_plan/meal_plan_page.dart';
import 'views/shopping_list_page.dart';

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
