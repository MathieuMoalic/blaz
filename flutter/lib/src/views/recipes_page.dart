import 'package:flutter/material.dart';
import '../api.dart';
import 'recipe_detail_page.dart';
import 'add_recipe_page.dart';

class RecipesPage extends StatefulWidget {
  const RecipesPage({super.key});
  @override
  State<RecipesPage> createState() => RecipesPageState();
}

class RecipesPageState extends State<RecipesPage> {
  late Future<List<Recipe>> _future;

  @override
  void initState() {
    super.initState();
    _future = fetchRecipes();
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

  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        _AppTitle(
          'Recipes',
          trailing: IconButton(
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
                final items = snap.data ?? const [];
                if (items.isEmpty) {
                  return const Center(child: Text('No recipes yet'));
                }
                return ListView.separated(
                  padding: const EdgeInsets.all(12),
                  itemCount: items.length,
                  itemBuilder: (_, i) {
                    final r = items[i];
                    final thumb = mediaUrl(r.imagePath);
                    return ListTile(
                      leading: thumb == null
                          ? const Icon(Icons.image_not_supported)
                          : ClipRRect(
                              borderRadius: BorderRadius.circular(6),
                              child: Image.network(
                                thumb,
                                width: 56,
                                height: 56,
                                fit: BoxFit.cover,
                              ),
                            ),
                      title: Text(r.title),
                      onTap: () async {
                        await Navigator.of(context).push(
                          MaterialPageRoute(
                            builder: (_) => RecipeDetailPage(recipeId: r.id),
                          ),
                        );
                        await refresh();
                      },
                      trailing: Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          IconButton(
                            tooltip: 'Assign to day',
                            icon: const Icon(Icons.event),
                            onPressed: () => _assignRecipe(r),
                          ),
                          IconButton(
                            tooltip: 'Delete',
                            icon: const Icon(Icons.delete_outline),
                            onPressed: () async {
                              await deleteRecipe(r.id);
                              await refresh();
                            },
                          ),
                        ],
                      ),
                    );
                  },
                  separatorBuilder: (_, __) => const Divider(height: 1),
                );
              },
            ),
          ),
        ),
      ],
    );
  }
}

class _AppTitle extends StatelessWidget {
  final String text;
  final Widget? trailing;
  const _AppTitle(this.text, {this.trailing, super.key});

  @override
  Widget build(BuildContext context) {
    return Container(
      height: 48,
      alignment: Alignment.centerLeft,
      padding: const EdgeInsets.symmetric(horizontal: 12),
      child: Row(
        children: [
          Expanded(
            child: Text(text, style: Theme.of(context).textTheme.titleLarge),
          ),
          if (trailing != null) trailing!,
        ],
      ),
    );
  }
}

