import 'package:flutter/material.dart';
import 'package:blaz/src/api.dart';

class RecipesPage extends StatefulWidget {
  const RecipesPage({super.key});
  @override
  RecipesPageState createState() => RecipesPageState();
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
      _future = f; // setState must be void
    });
    await f;
  }

  @override
  Widget build(BuildContext context) {
    return FutureBuilder<List<Recipe>>(
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
          itemBuilder: (_, i) => ListTile(title: Text(items[i].title)),
          separatorBuilder: (_, __) => const Divider(height: 1),
        );
      },
    );
  }
}
