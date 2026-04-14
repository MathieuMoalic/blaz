import 'package:flutter/material.dart';
import '../api.dart';

class CategoriesPage extends StatefulWidget {
  const CategoriesPage({super.key});

  @override
  State<CategoriesPage> createState() => _CategoriesPageState();
}

class _CategoriesPageState extends State<CategoriesPage> {
  late Future<List<ShoppingCategory>> _future;
  List<ShoppingCategory> _categories = [];

  @override
  void initState() {
    super.initState();
    _future = _load();
  }

  Future<List<ShoppingCategory>> _load() async {
    final cats = await fetchCategories();
    _categories = cats;
    return cats;
  }

  void _refresh() {
    setState(() => _future = _load());
  }

  Future<void> _addCategory() async {
    final name = await _showNameDialog(title: 'Add Category');
    if (name == null || name.trim().isEmpty) return;

    try {
      await createCategory(name.trim());
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Created "$name"')),
      );
      _refresh();
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Failed to create: $e')),
      );
    }
  }

  Future<void> _editCategory(ShoppingCategory cat) async {
    final name = await _showNameDialog(
      title: 'Rename Category',
      initialValue: cat.name,
    );
    if (name == null || name.trim().isEmpty || name.trim() == cat.name) return;

    try {
      await updateCategory(cat.id, name: name.trim());
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Renamed to "$name"')),
      );
      _refresh();
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Failed to rename: $e')),
      );
    }
  }

  Future<void> _deleteCategory(ShoppingCategory cat) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Delete category?'),
        content: Text(
          'Delete "${cat.name}"? Items using this category will need to be reassigned.',
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
            child: const Text('Delete'),
          ),
        ],
      ),
    );

    if (confirmed != true) return;

    try {
      await deleteCategory(cat.id);
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Deleted "${cat.name}"')),
      );
      _refresh();
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('$e')),
      );
    }
  }

  Future<String?> _showNameDialog({
    required String title,
    String? initialValue,
  }) async {
    final controller = TextEditingController(text: initialValue ?? '');
    return showDialog<String>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text(title),
        content: TextField(
          controller: controller,
          autofocus: true,
          textCapitalization: TextCapitalization.words,
          decoration: const InputDecoration(
            labelText: 'Category name',
            border: OutlineInputBorder(),
          ),
          onSubmitted: (val) => Navigator.pop(ctx, val),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(ctx),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(ctx, controller.text),
            child: const Text('Save'),
          ),
        ],
      ),
    );
  }

  void _onReorder(int oldIndex, int newIndex) {
    if (oldIndex < newIndex) newIndex -= 1;
    setState(() {
      final item = _categories.removeAt(oldIndex);
      _categories.insert(newIndex, item);
    });
    _saveOrder();
  }

  Future<void> _saveOrder() async {
    try {
      final ids = _categories.map((c) => c.id).toList();
      await reorderCategories(ids);
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Failed to save order: $e')),
      );
      _refresh();
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Shopping Categories'),
        actions: [
          IconButton(
            icon: const Icon(Icons.add),
            tooltip: 'Add category',
            onPressed: _addCategory,
          ),
        ],
      ),
      body: FutureBuilder<List<ShoppingCategory>>(
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

          if (_categories.isEmpty) {
            return const Center(
              child: Column(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  Icon(Icons.category_outlined, size: 64, color: Colors.grey),
                  SizedBox(height: 16),
                  Text(
                    'No categories',
                    style: TextStyle(fontSize: 18, color: Colors.grey),
                  ),
                  SizedBox(height: 8),
                  Text(
                    'Tap + to add a category',
                    style: TextStyle(color: Colors.grey),
                  ),
                ],
              ),
            );
          }

          return ReorderableListView.builder(
            onReorder: _onReorder,
            itemCount: _categories.length,
            itemBuilder: (context, index) {
              final cat = _categories[index];
              final isOther = cat.name == 'Other';

              return Card(
                key: ValueKey(cat.id),
                margin: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
                child: ListTile(
                  leading: ReorderableDragStartListener(
                    index: index,
                    child: const Icon(Icons.drag_handle),
                  ),
                  title: Text(cat.name),
                  trailing: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      IconButton(
                        icon: const Icon(Icons.edit),
                        tooltip: 'Rename',
                        onPressed: () => _editCategory(cat),
                      ),
                      IconButton(
                        icon: const Icon(Icons.delete),
                        tooltip: isOther ? 'Cannot delete "Other"' : 'Delete',
                        color: isOther ? Colors.grey : Theme.of(context).colorScheme.error,
                        onPressed: isOther ? null : () => _deleteCategory(cat),
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
