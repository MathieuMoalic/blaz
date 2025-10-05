import 'package:flutter/material.dart';
import 'src/api.dart';

void main() => runApp(const BlazApp());

class BlazApp extends StatelessWidget {
  const BlazApp({super.key});
  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Blaz',
      theme: ThemeData(useMaterial3: true, colorSchemeSeed: Colors.teal),
      home: const RecipesPage(),
    );
  }
}

class RecipesPage extends StatefulWidget {
  const RecipesPage({super.key});
  @override
  State<RecipesPage> createState() => _RecipesPageState();
}

class _RecipesPageState extends State<RecipesPage> {
  late Future<List<Recipe>> _future;

  @override
  void initState() {
    super.initState();
    _future = fetchRecipes();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Blaz')),
      body: FutureBuilder<List<Recipe>>(
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
      ),
    );
  }
}

