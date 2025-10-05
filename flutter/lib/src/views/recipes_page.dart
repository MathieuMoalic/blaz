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
              // Push add screen; if it returns true, refresh list
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
                    return ListTile(
                      title: Text(r.title),
                      onTap: () async {
                        // Open detail; refresh after coming back (in case of edits later)
                        await Navigator.of(context).push(
                          MaterialPageRoute(
                            builder: (_) => RecipeDetailPage(recipeId: r.id),
                          ),
                        );
                        await refresh();
                      },
                      trailing: IconButton(
                        icon: const Icon(Icons.delete_outline),
                        onPressed: () async {
                          await deleteRecipe(
                            r.id,
                          ); // make sure this exists in api.dart
                          await refresh();
                        },
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
