import 'package:flutter/material.dart';
import '../api.dart';

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
        const _AppTitle('Recipes'),
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
                  itemBuilder: (_, i) => ListTile(
                    title: Text(items[i].title),
                    trailing: IconButton(
                      icon: const Icon(Icons.delete_outline),
                      onPressed: () async {
                        await deleteRecipe(items[i].id);
                        refresh();
                      },
                    ),
                  ),
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
  const _AppTitle(this.text);
  @override
  Widget build(BuildContext context) {
    return Container(
      height: 48,
      alignment: Alignment.centerLeft,
      padding: const EdgeInsets.symmetric(horizontal: 12),
      child: Text(text, style: Theme.of(context).textTheme.titleLarge),
    );
  }
}
