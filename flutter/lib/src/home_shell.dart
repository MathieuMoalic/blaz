import 'package:flutter/material.dart';
import 'views/recipes_page.dart';
import 'views/meal_plan/meal_plan_page.dart';
import 'views/shopping_list_page.dart';

class HomeShell extends StatefulWidget {
  const HomeShell({super.key});
  @override
  State<HomeShell> createState() => _HomeShellState();
}

class _TabNavObserver extends NavigatorObserver {
  final VoidCallback onStackChanged;
  _TabNavObserver(this.onStackChanged);

  void _notifyLater() {
    WidgetsBinding.instance.addPostFrameCallback((_) => onStackChanged());
  }

  @override
  void didPush(Route route, Route? previousRoute) => _notifyLater();
  @override
  void didPop(Route route, Route? previousRoute) => _notifyLater();
  @override
  void didRemove(Route route, Route? previousRoute) => _notifyLater();
  @override
  void didReplace({Route? newRoute, Route? oldRoute}) => _notifyLater();
}

class _HomeShellState extends State<HomeShell> {
  int _index = 0;

  final _recipesKey = GlobalKey<RecipesPageState>();
  final _navKeys = List.generate(3, (_) => GlobalKey<NavigatorState>());
  late final List<_TabNavObserver> _observers;

  @override
  void initState() {
    super.initState();
    _observers = List.generate(3, (_) => _TabNavObserver(_markDirty));
  }

  void _markDirty() {
    if (!mounted) return;
    setState(() {}); // now runs in the post-frame callback
  }

  NavigatorState get _currentNav => _navKeys[_index].currentState!;

  Widget _buildTabNavigator(int i) {
    return Offstage(
      offstage: _index != i,
      child: Navigator(
        key: _navKeys[i],
        observers: [_observers[i]],
        onGenerateRoute: (settings) {
          Widget page;
          switch (i) {
            case 0:
              page = RecipesPage(key: _recipesKey);
              break;
            case 1:
              page = const MealPlanPage();
              break;
            default:
              page = const ShoppingListPage();
          }
          return MaterialPageRoute(builder: (_) => page, settings: settings);
        },
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final canPopNested = _navKeys[_index].currentState?.canPop() ?? false;

    return PopScope(
      // Let the system pop only when the tab's stack is at root.
      canPop: !canPopNested,
      onPopInvokedWithResult: (didPop, result) {
        if (didPop) return; // system already popped the route
        if (_currentNav.canPop()) {
          _currentNav.pop(result); // pop inside the current tab
        }
      },
      child: Scaffold(
        body: SafeArea(
          child: Stack(children: List.generate(3, _buildTabNavigator)),
        ),
        bottomNavigationBar: NavigationBar(
          selectedIndex: _index,
          onDestinationSelected: (i) => setState(() => _index = i),
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
        ),
      ),
    );
  }
}
