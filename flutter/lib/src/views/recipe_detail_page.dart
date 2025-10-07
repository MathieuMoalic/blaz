import 'package:flutter/material.dart';
import '../api.dart';
import 'edit_recipe_page.dart';

class RecipeDetailPage extends StatefulWidget {
  final int recipeId;
  const RecipeDetailPage({super.key, required this.recipeId});

  @override
  State<RecipeDetailPage> createState() => _RecipeDetailPageState();
}

class _RecipeDetailPageState extends State<RecipeDetailPage> {
  late Future<Recipe> _future;

  @override
  void initState() {
    super.initState();
    _future = fetchRecipe(widget.recipeId);
  }

  Future<void> _refresh() async {
    final f = fetchRecipe(widget.recipeId);
    setState(() => _future = f);
    await f;
  }

  Future<void> _confirmDelete(Recipe r) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Delete recipe?'),
        content: Text('Are you sure you want to delete “${r.title}”?'),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(ctx, false),
            child: const Text('Cancel'),
          ),
          FilledButton.tonalIcon(
            onPressed: () => Navigator.pop(ctx, true),
            icon: const Icon(Icons.delete_outline),
            label: const Text('Delete'),
          ),
        ],
      ),
    );
    if (ok == true) {
      try {
        await deleteRecipe(r.id);
        if (!mounted) return;
        // pop back to the list and signal that something changed
        Navigator.of(context).pop(true);
        ScaffoldMessenger.of(
          context,
        ).showSnackBar(SnackBar(content: Text('Deleted “${r.title}”')));
      } catch (e) {
        if (!mounted) return;
        ScaffoldMessenger.of(
          context,
        ).showSnackBar(SnackBar(content: Text('Failed to delete: $e')));
      }
    }
  }

  Future<void> _addIngredients(Recipe r) async {
    if (r.ingredients.isEmpty) {
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(const SnackBar(content: Text('No ingredients to add')));
      return;
    }

    final selected = await _pickIngredientsBottomSheet(
      title: 'Add to shopping list',
      items: r.ingredients,
    );

    if (selected == null || selected.isEmpty) return;

    try {
      // Adjust if your API name/signature differs:
      await addShoppingItems(selected);
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text(
            'Added ${selected.length} item(s) to the shopping list',
          ),
        ),
      );
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(SnackBar(content: Text('Failed to add: $e')));
    }
  }

  Future<List<String>?> _pickIngredientsBottomSheet({
    required String title,
    required List<String> items,
  }) async {
    return showModalBottomSheet<List<String>>(
      context: context,
      isScrollControlled: true,
      showDragHandle: true,
      builder: (ctx) {
        final media = MediaQuery.of(ctx);
        final height = media.size.height * 0.7;

        // local mutable state for the bottom sheet
        final selections = List<bool>.filled(items.length, true);
        bool allSelected() => selections.every((v) => v);
        bool anySelected() => selections.any((v) => v);

        return StatefulBuilder(
          builder: (ctx, setSheetState) {
            final triValue = allSelected()
                ? true
                : anySelected()
                ? null
                : false;

            return SafeArea(
              child: SizedBox(
                height: height,
                child: Column(
                  children: [
                    Padding(
                      padding: const EdgeInsets.fromLTRB(16, 10, 16, 6),
                      child: Row(
                        children: [
                          Expanded(
                            child: Text(
                              title,
                              style: Theme.of(ctx).textTheme.titleMedium,
                            ),
                          ),
                          const SizedBox(width: 8),
                          // Select all / none (tri-state)
                          Row(
                            children: [
                              const Text('Select all'),
                              Checkbox(
                                value: triValue,
                                tristate: true,
                                onChanged: (_) {
                                  final target = !(allSelected());
                                  setSheetState(() {
                                    for (
                                      var i = 0;
                                      i < selections.length;
                                      i++
                                    ) {
                                      selections[i] = target;
                                    }
                                  });
                                },
                              ),
                            ],
                          ),
                        ],
                      ),
                    ),
                    const Divider(height: 1),
                    Expanded(
                      child: ListView.builder(
                        padding: const EdgeInsets.symmetric(vertical: 4),
                        itemCount: items.length,
                        itemBuilder: (_, i) => CheckboxListTile(
                          value: selections[i],
                          onChanged: (v) =>
                              setSheetState(() => selections[i] = v ?? false),
                          title: Text(items[i]),
                          controlAffinity: ListTileControlAffinity.leading,
                          dense: true,
                        ),
                      ),
                    ),
                    const Divider(height: 1),
                    Padding(
                      padding: EdgeInsets.only(
                        left: 16,
                        right: 16,
                        top: 10,
                        bottom: media.viewInsets.bottom + 12,
                      ),
                      child: Row(
                        children: [
                          Expanded(
                            child: OutlinedButton(
                              onPressed: () => Navigator.pop(ctx, <String>[]),
                              child: const Text('Cancel'),
                            ),
                          ),
                          const SizedBox(width: 12),
                          Expanded(
                            child: FilledButton.icon(
                              onPressed: anySelected()
                                  ? () {
                                      final picked = <String>[];
                                      for (var i = 0; i < items.length; i++) {
                                        if (selections[i]) picked.add(items[i]);
                                      }
                                      Navigator.pop(ctx, picked);
                                    }
                                  : null,
                              icon: const Icon(Icons.shopping_cart_outlined),
                              label: Text(
                                anySelected()
                                    ? 'Add ${selections.where((s) => s).length}'
                                    : 'Add',
                              ),
                            ),
                          ),
                        ],
                      ),
                    ),
                  ],
                ),
              ),
            );
          },
        );
      },
    );
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Recipe'),
        actions: [
          IconButton(
            tooltip: 'Add ingredients to shopping list',
            icon: const Icon(Icons.shopping_cart_outlined),
            onPressed: () async {
              final r = await _future;
              if (!mounted) return;
              _addIngredients(r);
            },
          ),
          IconButton(
            tooltip: 'Edit',
            icon: const Icon(Icons.edit_outlined),
            onPressed: () async {
              final r = await _future;
              final changed = await Navigator.push<bool>(
                context,
                MaterialPageRoute(builder: (_) => EditRecipePage(recipe: r)),
              );
              if (changed == true) _refresh();
            },
          ),
          IconButton(
            tooltip: 'Delete',
            icon: const Icon(Icons.delete_outline),
            onPressed: () async {
              final r = await _future;
              if (!mounted) return;
              _confirmDelete(r);
            },
          ),
        ],
      ),
      body: FutureBuilder<Recipe>(
        future: _future,
        builder: (context, snap) {
          if (snap.connectionState != ConnectionState.done) {
            return const Center(child: CircularProgressIndicator());
          }
          if (snap.hasError) {
            return Center(child: Text('Error: ${snap.error}'));
          }
          final r = snap.data!;
          final img = mediaUrl(r.imagePath);
          return RefreshIndicator(
            onRefresh: _refresh,
            child: ListView(
              padding: const EdgeInsets.all(16),
              children: [
                Text(r.title, style: Theme.of(context).textTheme.headlineSmall),
                const SizedBox(height: 8),
                if (img != null) ...[
                  ClipRRect(
                    borderRadius: BorderRadius.circular(10),
                    child: SizedBox(
                      height: 200,
                      child: Ink.image(
                        image: NetworkImage(img),
                        fit: BoxFit.cover,
                      ),
                    ),
                  ),
                  const SizedBox(height: 12),
                ],
                _MetaRow(
                  label: 'Source',
                  value: r.source.isEmpty ? '—' : r.source,
                ),
                _MetaRow(
                  label: 'Yield',
                  value: r.yieldText.isEmpty ? '—' : r.yieldText,
                ),
                _MetaRow(
                  label: 'Created',
                  value: r.createdAt.isEmpty ? '—' : r.createdAt,
                ),
                _MetaRow(
                  label: 'Updated',
                  value: r.updatedAt.isEmpty ? '—' : r.updatedAt,
                ),
                const SizedBox(height: 16),
                if (r.notes.isNotEmpty) ...[
                  Text('Notes', style: Theme.of(context).textTheme.titleMedium),
                  const SizedBox(height: 6),
                  Text(r.notes),
                  const SizedBox(height: 16),
                ],
                Text(
                  'Ingredients',
                  style: Theme.of(context).textTheme.titleMedium,
                ),
                const SizedBox(height: 6),
                if (r.ingredients.isEmpty)
                  const Text('—')
                else
                  ...r.ingredients.map((s) => _Bullet(s)).toList(),
                const SizedBox(height: 16),
                Text(
                  'Instructions',
                  style: Theme.of(context).textTheme.titleMedium,
                ),
                const SizedBox(height: 6),
                if (r.instructions.isEmpty)
                  const Text('—')
                else ...[
                  for (int i = 0; i < r.instructions.length; i++)
                    _Numbered(step: i + 1, text: r.instructions[i]),
                ],
              ],
            ),
          );
        },
      ),
    );
  }
}

class _MetaRow extends StatelessWidget {
  final String label;
  final String value;
  const _MetaRow({required this.label, required this.value});
  @override
  Widget build(BuildContext context) {
    final styleLabel = Theme.of(context).textTheme.bodySmall;
    final styleValue = Theme.of(context).textTheme.bodyMedium;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 2),
      child: Row(
        children: [
          SizedBox(width: 90, child: Text(label, style: styleLabel)),
          Expanded(child: Text(value, style: styleValue)),
        ],
      ),
    );
  }
}

class _Bullet extends StatelessWidget {
  final String text;
  const _Bullet(this.text);
  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const Text('•  '),
          Expanded(child: Text(text)),
        ],
      ),
    );
  }
}

class _Numbered extends StatelessWidget {
  final int step;
  final String text;
  const _Numbered({required this.step, required this.text});
  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 6),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text('$step. '),
          Expanded(child: Text(text)),
        ],
      ),
    );
  }
}
