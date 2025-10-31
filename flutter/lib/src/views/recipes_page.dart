import 'dart:async';
import 'package:flutter/material.dart';
import '../api.dart';
import 'recipe_detail_page.dart';
import 'add_recipe_page.dart';
import 'login_page.dart';
import '../auth.dart';
import '../widgets/app_title.dart';
import 'app_state_page.dart';

enum _MenuAction { settings, logout }

class RecipesPage extends StatefulWidget {
  const RecipesPage({super.key});
  @override
  State<RecipesPage> createState() => RecipesPageState();
}

class RecipesPageState extends State<RecipesPage> {
  late Future<List<Recipe>> _future;
  final _filterCtrl = TextEditingController();
  String _query = '';
  Timer? _debounce;

  @override
  void initState() {
    super.initState();
    _future = fetchRecipes();
    _filterCtrl.addListener(_onFilterChanged);
  }

  @override
  void dispose() {
    _filterCtrl.removeListener(_onFilterChanged);
    _filterCtrl.dispose();
    _debounce?.cancel();
    super.dispose();
  }

  void _onFilterChanged() {
    _debounce?.cancel();
    _debounce = Timer(const Duration(milliseconds: 200), () {
      if (!mounted) return;
      setState(() => _query = _filterCtrl.text.trim().toLowerCase());
    });
  }

  Future<void> refresh() async {
    final f = fetchRecipes();
    setState(() {
      _future = f;
    });
    await f;
  }

  String _ymd(DateTime d) =>
      '${d.year.toString().padLeft(4, '0')}-${d.month.toString().padLeft(2, '0')}-${d.day.toString().padLeft(2, '0')}';

  Future<void> _assignRecipe(Recipe r) async {
    final now = DateTime.now();
    final picked = await showDatePicker(
      context: context,
      initialDate: now,
      firstDate: DateTime(now.year - 1),
      lastDate: DateTime(now.year + 2),
    );
    if (picked == null) return;

    final day = _ymd(picked);
    try {
      final entry = await assignRecipeToDay(day: day, recipeId: r.id);
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Assigned "${r.title}" to ${entry.day}')),
      );
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(SnackBar(content: Text('Failed to assign: $e')));
    }
  }

  bool _matches(Recipe r, String q) {
    if (q.isEmpty) return true;
    final needle = q.toLowerCase();

    if (r.title.toLowerCase().contains(needle)) return true;

    for (final ing in r.ingredients) {
      if (ing.name.toLowerCase().contains(needle)) return true;
      if (ing.toLine().toLowerCase().contains(needle)) return true;
    }

    for (final step in r.instructions) {
      if (step.toLowerCase().contains(needle)) return true;
    }

    return false;
  }

  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        AppTitle(
          'Recipes',
          trailing: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              IconButton(
                tooltip: 'Add recipe',
                icon: const Icon(Icons.add),
                onPressed: () async {
                  final created = await Navigator.of(context).push<bool>(
                    MaterialPageRoute(builder: (_) => const AddRecipePage()),
                  );
                  if (created == true) {
                    await refresh();
                  }
                },
              ),
              PopupMenuButton<_MenuAction>(
                tooltip: 'Account',
                icon: const Icon(Icons.account_circle),
                onSelected: (choice) async {
                  switch (choice) {
                    case _MenuAction.settings:
                      await Navigator.of(context).push(
                        MaterialPageRoute(builder: (_) => const AppStatePage()),
                      );
                      break;
                    case _MenuAction.logout:
                      await Auth.logout();
                      if (!context.mounted) return;
                      Navigator.of(context).pushAndRemoveUntil(
                        MaterialPageRoute(builder: (_) => const LoginPage()),
                        (_) => false,
                      );
                      break;
                  }
                },
                itemBuilder: (context) => const [
                  PopupMenuItem<_MenuAction>(
                    value: _MenuAction.settings,
                    child: ListTile(
                      leading: Icon(Icons.settings),
                      title: Text('App settings'),
                      dense: true,
                      contentPadding: EdgeInsets.zero,
                    ),
                  ),
                ],
              ),
            ],
          ),
        ),

        // Filter field
        Padding(
          padding: const EdgeInsets.fromLTRB(12, 0, 12, 8),
          child: TextField(
            controller: _filterCtrl,
            textInputAction: TextInputAction.search,
            decoration: InputDecoration(
              hintText: 'Filter by title or ingredient',
              prefixIcon: const Icon(Icons.search),
              isDense: true,
              suffixIcon: _query.isEmpty
                  ? null
                  : IconButton(
                      tooltip: 'Clear',
                      icon: const Icon(Icons.close),
                      onPressed: () {
                        _filterCtrl.clear();
                        FocusScope.of(context).unfocus();
                      },
                    ),
              border: OutlineInputBorder(
                borderRadius: BorderRadius.circular(12),
              ),
            ),
          ),
        ),

        Expanded(
          child: RefreshIndicator(
            onRefresh: refresh,
            child: FutureBuilder<List<Recipe>>(
              future: _future,
              builder: (context, snap) {
                if (snap.connectionState != ConnectionState.done) {
                  return const Center(child: CircularProgressIndicator());
                }
                if (snap.hasError) {
                  return Center(child: Text('Error: ${snap.error}'));
                }
                final items = snap.data ?? const <Recipe>[];
                final filtered = items
                    .where((r) => _matches(r, _query))
                    .toList();
                if (filtered.isEmpty) {
                  return const _EmptyState();
                }

                return LayoutBuilder(
                  builder: (context, c) {
                    int cols = 2;
                    final w = c.maxWidth;
                    if (w >= 1200) {
                      cols = 5;
                    } else if (w >= 900) {
                      cols = 4;
                    } else if (w >= 600) {
                      cols = 3;
                    }

                    return GridView.builder(
                      padding: const EdgeInsets.fromLTRB(12, 4, 12, 12),
                      gridDelegate: SliverGridDelegateWithFixedCrossAxisCount(
                        crossAxisCount: cols,
                        mainAxisSpacing: 12,
                        crossAxisSpacing: 12,
                        childAspectRatio: 3 / 4,
                      ),
                      itemCount: filtered.length,
                      itemBuilder: (_, i) {
                        final r = filtered[i];
                        final thumb = mediaUrl(
                          r.imagePathSmall ?? r.imagePathFull,
                        );
                        return _RecipeCard(
                          title: r.title,
                          imageUrl: thumb,
                          onOpen: () async {
                            await Navigator.of(context).push(
                              MaterialPageRoute(
                                builder: (_) =>
                                    RecipeDetailPage(recipeId: r.id),
                              ),
                            );
                            await refresh();
                          },
                          onAssign: () => _assignRecipe(r),
                        );
                      },
                    );
                  },
                );
              },
            ),
          ),
        ),
      ],
    );
  }
}

class _RecipeCard extends StatelessWidget {
  final String title;
  final String? imageUrl;
  final VoidCallback? onOpen;
  final VoidCallback? onAssign;

  const _RecipeCard({
    required this.title,
    required this.imageUrl,
    this.onOpen,
    this.onAssign,
  });

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return InkWell(
      onTap: onOpen,
      borderRadius: BorderRadius.circular(16),
      child: Card(
        clipBehavior: Clip.antiAlias,
        elevation: 2,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(16)),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            // Image with a tucked-away action in the corner
            Expanded(
              child: Stack(
                fit: StackFit.expand,
                children: [
                  imageUrl == null
                      ? _placeholder()
                      : Image.network(
                          imageUrl!,
                          fit: BoxFit.cover,
                          frameBuilder: (context, child, frame, wasSync) {
                            if (wasSync || frame != null) return child;
                            return const Center(
                              child: CircularProgressIndicator(),
                            );
                          },
                          errorBuilder: (_, __, ___) => _placeholder(),
                        ),
                  Positioned(
                    top: 8,
                    right: 8,
                    child: Material(
                      color: Colors.black54,
                      shape: const CircleBorder(),
                      child: IconButton(
                        tooltip: 'Assign to day',
                        icon: const Icon(
                          Icons.event,
                          size: 20,
                          color: Colors.white,
                        ),
                        onPressed: onAssign,
                        splashRadius: 22,
                      ),
                    ),
                  ),
                ],
              ),
            ),
            Padding(
              padding: const EdgeInsets.all(12),
              child: Text(
                title,
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
                textAlign: TextAlign.center,
                style: theme.textTheme.titleMedium,
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _placeholder() {
    return Container(
      alignment: Alignment.center,
      child: const Icon(Icons.restaurant_menu, size: 48),
    );
  }
}

class _EmptyState extends StatelessWidget {
  const _EmptyState();

  @override
  Widget build(BuildContext context) {
    return ListView(
      physics: const AlwaysScrollableScrollPhysics(),
      children: const [
        SizedBox(height: 120),
        Icon(Icons.no_food, size: 48),
        SizedBox(height: 12),
        Center(
          child: Text(
            'No recipes match your filter.',
            textAlign: TextAlign.center,
          ),
        ),
      ],
    );
  }
}
