import 'package:flutter/material.dart';
import '../api.dart';

class DeletedRecipesPage extends StatefulWidget {
  const DeletedRecipesPage({super.key});

  @override
  State<DeletedRecipesPage> createState() => _DeletedRecipesPageState();
}

class _DeletedRecipesPageState extends State<DeletedRecipesPage> {
  late Future<List<Recipe>> _future;

  @override
  void initState() {
    super.initState();
    _future = fetchDeletedRecipes();
  }

  void _refresh() {
    setState(() => _future = fetchDeletedRecipes());
  }

  Future<void> _restore(Recipe recipe) async {
    try {
      await restoreRecipe(recipe.id);
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Restored "${recipe.title}"')),
      );
      _refresh();
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Failed to restore: $e')),
      );
    }
  }

  Future<void> _permanentDelete(Recipe recipe) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Permanently delete?'),
        content: Text(
          'This will permanently delete "${recipe.title}". This cannot be undone.',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(ctx, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(ctx, true),
            style: FilledButton.styleFrom(
              backgroundColor: Theme.of(ctx).colorScheme.error,
            ),
            child: const Text('Delete forever'),
          ),
        ],
      ),
    );

    if (confirmed != true) return;

    try {
      await permanentDeleteRecipe(recipe.id);
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Permanently deleted "${recipe.title}"')),
      );
      _refresh();
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Failed to delete: $e')),
      );
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Recently Deleted'),
      ),
      body: FutureBuilder<List<Recipe>>(
        future: _future,
        builder: (context, snapshot) {
          if (snapshot.connectionState == ConnectionState.waiting) {
            return const Center(child: CircularProgressIndicator());
          }

          if (snapshot.hasError) {
            return Center(
              child: Column(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  Text('Error: ${snapshot.error}'),
                  const SizedBox(height: 16),
                  FilledButton(
                    onPressed: _refresh,
                    child: const Text('Retry'),
                  ),
                ],
              ),
            );
          }

          final recipes = snapshot.data ?? [];

          if (recipes.isEmpty) {
            return const Center(
              child: Column(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  Icon(Icons.delete_outline, size: 64, color: Colors.grey),
                  SizedBox(height: 16),
                  Text(
                    'No deleted recipes',
                    style: TextStyle(fontSize: 18, color: Colors.grey),
                  ),
                  SizedBox(height: 8),
                  Text(
                    'Deleted recipes will appear here',
                    style: TextStyle(color: Colors.grey),
                  ),
                ],
              ),
            );
          }

          return ListView.builder(
            itemCount: recipes.length,
            itemBuilder: (context, index) {
              final recipe = recipes[index];
              final thumb = mediaUrl(recipe.imagePathSmall);

              return Card(
                margin: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
                child: ListTile(
                  leading: ClipRRect(
                    borderRadius: BorderRadius.circular(8),
                    child: SizedBox(
                      width: 56,
                      height: 56,
                      child: thumb != null
                          ? Image.network(thumb, fit: BoxFit.cover)
                          : Container(
                              color: Colors.grey[800],
                              child: const Icon(Icons.restaurant, color: Colors.grey),
                            ),
                    ),
                  ),
                  title: Text(recipe.title),
                  subtitle: Text(
                    '${recipe.ingredients.length} ingredients',
                    style: TextStyle(color: Colors.grey[400]),
                  ),
                  trailing: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      IconButton(
                        icon: const Icon(Icons.restore),
                        tooltip: 'Restore',
                        onPressed: () => _restore(recipe),
                      ),
                      IconButton(
                        icon: const Icon(Icons.delete_forever),
                        tooltip: 'Delete forever',
                        color: Theme.of(context).colorScheme.error,
                        onPressed: () => _permanentDelete(recipe),
                      ),
                    ],
                  ),
                ),
              );
            },
          );
        },
      ),
    );
  }
}
