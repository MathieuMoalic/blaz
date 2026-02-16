import 'dart:async';
import 'package:flutter/material.dart';
import '../api.dart';
import 'recipe_detail_page.dart';
import 'add_recipe_page.dart';
import '../widgets/app_title.dart';
import 'app_state_page.dart';

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

  // cache + soft loading flag
  List<Recipe> _cache = const <Recipe>[];
  bool _refreshing = false;

  @override
  void initState() {
    super.initState();
    _future = _loadInitial();
    _filterCtrl.addListener(_onFilterChanged);
  }

  Future<List<Recipe>> _loadInitial() async {
    final list = await fetchRecipes();
    _cache = list;
    return list;
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

  /// Refresh without blanking the grid.
  Future<void> refresh() async {
    if (_refreshing) return;
    setState(() => _refreshing = true);

    try {
      final list = await fetchRecipes();
      if (!mounted) return;

      setState(() {
        _cache = list;
        // Important: set to an already-completed future to avoid flicker.
        _future = Future.value(list);
      });
    } finally {
      if (mounted) {
        setState(() => _refreshing = false);
      }
    }
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
      locale: const Locale('en', 'GB'), // UK locale starts weeks on Monday
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

  Future<void> _onAddRecipe() async {
    final created = await Navigator.of(
      context,
    ).push<bool>(MaterialPageRoute(builder: (_) => const AddRecipePage()));
    if (created == true) {
      await refresh();
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
                tooltip: 'App settings',
                icon: const Icon(Icons.settings), // cog wheel
                onPressed: () async {
                  await Navigator.of(context).push(
                    MaterialPageRoute(builder: (_) => const AppStatePage()),
                  );
                },
              ),
            ],
          ),
        ),

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
          child: Stack(
            children: [
              RefreshIndicator(
                onRefresh: refresh,
                child: FutureBuilder<List<Recipe>>(
                  future: _future,
                  builder: (context, snap) {
                    // Use cache during loading to avoid flicker.
                    final items =
                        (snap.connectionState == ConnectionState.done &&
                            snap.data != null)
                        ? snap.data!
                        : _cache;

                    if (items.isEmpty) {
                      if (snap.hasError) {
                        return Center(child: Text('Error: ${snap.error}'));
                      }
                      if (snap.connectionState != ConnectionState.done) {
                        return const Center(child: CircularProgressIndicator());
                      }
                      return const _EmptyState();
                    }

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
                          gridDelegate:
                              SliverGridDelegateWithFixedCrossAxisCount(
                                crossAxisCount: cols,
                                mainAxisSpacing: 12,
                                crossAxisSpacing: 12,
                                childAspectRatio: 3 / 4,
                              ),
                          itemCount: filtered.length,
                          itemBuilder: (_, i) {
                            final r = filtered[i];
                            final thumb = mediaUrl(r.imagePathSmall);

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

              // Subtle top loading bar instead of full-screen spinner.
              if (_refreshing)
                const Positioned(
                  left: 0,
                  right: 0,
                  top: 0,
                  child: LinearProgressIndicator(minHeight: 2),
                ),

              // Floating "Add recipe" button in lower right
              Positioned(
                right: 16,
                bottom: 16,
                child: FloatingActionButton(
                  onPressed: _onAddRecipe,
                  tooltip: 'Add recipe',
                  child: const Icon(Icons.add),
                ),
              ),
            ],
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
