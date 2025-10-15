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

  final Set<int> _checkedIngredients = {};
  final Set<int> _checkedSteps = {};
  double _scale = 1.0;

  void _toggleIngredient(int i) {
    setState(() {
      if (!_checkedIngredients.add(i)) _checkedIngredients.remove(i);
    });
  }

  void _toggleStep(int i) {
    setState(() {
      if (!_checkedSteps.add(i)) _checkedSteps.remove(i);
    });
  }

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
      items: r.ingredients, // pass Ingredient objects
    );
    if (selected == null || selected.isEmpty) return;
    await addShoppingItems(selected);

    try {
      await addShoppingItems(selected);
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Added ${selected.length} item(s)')),
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
    required List<Ingredient> items,
  }) async {
    return showModalBottomSheet<List<String>>(
      context: context,
      isScrollControlled: true,
      showDragHandle: true,
      builder: (ctx) {
        final media = MediaQuery.of(ctx);
        final height = media.size.height * 0.7;

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
                          Row(
                            children: [
                              const Text('Select all'),
                              Checkbox(
                                value: triValue,
                                tristate: true,
                                onChanged: (_) {
                                  final target = !allSelected();
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
                          title: Text(items[i].toLine(factor: _scale)),
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
                                        if (selections[i]) {
                                          picked.add(
                                            items[i].toLine(factor: _scale),
                                          );
                                        }
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

  void _openImageViewer({required String fullUrl, required String heroTag}) {
    Navigator.of(context, rootNavigator: true).push(
      PageRouteBuilder(
        opaque: false,
        barrierColor: Colors.black.withOpacity(0.95),
        pageBuilder: (_, __, ___) =>
            _ImageViewerPage(url: fullUrl, heroTag: heroTag),
        transitionsBuilder: (_, anim, __, child) =>
            FadeTransition(opacity: anim, child: child),
      ),
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
          final small = mediaUrl(r.imagePathSmall);
          final full = mediaUrl(r.imagePathFull);
          final heroTag = 'recipe-image-${r.id}';

          return RefreshIndicator(
            onRefresh: _refresh,
            child: ListView(
              padding: const EdgeInsets.all(16),
              children: [
                Text(r.title, style: Theme.of(context).textTheme.headlineSmall),
                const SizedBox(height: 8),
                if (small != null) ...[
                  Hero(
                    tag: heroTag,
                    child: Material(
                      borderRadius: BorderRadius.circular(10),
                      clipBehavior: Clip.antiAlias,
                      child: InkWell(
                        onTap: () => _openImageViewer(
                          fullUrl: full ?? small,
                          heroTag: heroTag,
                        ),
                        child: Ink.image(
                          image: NetworkImage(small),
                          fit: BoxFit.cover,
                          height: 250,
                          width: double.infinity,
                        ),
                      ),
                    ),
                  ),
                  const SizedBox(height: 12),
                ],

                // Ingredients + scale
                const SizedBox(height: 16),
                Text(
                  'Ingredients',
                  style: Theme.of(context).textTheme.titleMedium,
                ),
                const SizedBox(height: 6),
                Row(
                  children: [
                    Text(
                      'Scale',
                      style: Theme.of(context).textTheme.titleMedium,
                    ),
                    const SizedBox(width: 10),
                    DropdownButton<double>(
                      value: _scale,
                      onChanged: (v) => setState(() => _scale = v ?? 1.0),
                      items: const [0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0]
                          .map(
                            (v) => DropdownMenuItem(
                              value: v,
                              child: Text('${v}x'),
                            ),
                          )
                          .toList(),
                    ),
                    const SizedBox(width: 8),
                    TextButton(
                      onPressed: () => setState(() => _scale = 1.0),
                      child: const Text('Reset'),
                    ),
                  ],
                ),
                const SizedBox(height: 6),
                if (r.ingredients.isEmpty)
                  const Text('—')
                else
                  ...r.ingredients.asMap().entries.map((e) {
                    final idx = e.key;
                    final ing = e.value;
                    final line = ing.toLine(factor: _scale);
                    final checked = _checkedIngredients.contains(idx);
                    return _Bullet(
                      text: line,
                      checked: checked,
                      onTap: () => _toggleIngredient(idx),
                    );
                  }),

                // Instructions
                const SizedBox(height: 16),
                Text(
                  'Instructions',
                  style: Theme.of(context).textTheme.titleMedium,
                ),
                const SizedBox(height: 6),
                if (r.instructions.isEmpty)
                  const Text('—')
                else
                  for (int i = 0; i < r.instructions.length; i++)
                    _Numbered(
                      step: i + 1,
                      text: r.instructions[i],
                      checked: _checkedSteps.contains(i),
                      onTap: () => _toggleStep(i),
                    ),

                // Meta
                if (r.notes.isNotEmpty) ...[
                  const SizedBox(height: 16),
                  Text('Notes', style: Theme.of(context).textTheme.titleMedium),
                  const SizedBox(height: 6),
                  Text(r.notes),
                  const SizedBox(height: 6),
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
  final bool checked;
  final VoidCallback onTap;
  const _Bullet({
    required this.text,
    required this.checked,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final base = Theme.of(context).textTheme.bodyMedium;
    final style = base?.copyWith(
      decoration: checked ? TextDecoration.lineThrough : null,
      color: checked
          ? (base?.color ?? Colors.black).withOpacity(0.55)
          : base?.color,
    );
    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(6),
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 6),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const Text('•  '),
            Expanded(
              child: AnimatedDefaultTextStyle(
                duration: const Duration(milliseconds: 120),
                style: style ?? const TextStyle(),
                child: Text(text),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _Numbered extends StatelessWidget {
  final int step;
  final String text;
  final bool checked;
  final VoidCallback onTap;
  const _Numbered({
    required this.step,
    required this.text,
    required this.checked,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final base = Theme.of(context).textTheme.bodyMedium;
    final style = base?.copyWith(
      decoration: checked ? TextDecoration.lineThrough : null,
      color: checked
          ? (base?.color ?? Colors.black).withOpacity(0.55)
          : base?.color,
    );
    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(6),
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 8),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('$step. '),
            Expanded(
              child: AnimatedDefaultTextStyle(
                duration: const Duration(milliseconds: 120),
                style: style ?? const TextStyle(),
                child: Text(text),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _ImageViewerPage extends StatefulWidget {
  final String url;
  final String heroTag;
  const _ImageViewerPage({required this.url, required this.heroTag});

  @override
  State<_ImageViewerPage> createState() => _ImageViewerPageState();
}

class _ImageViewerPageState extends State<_ImageViewerPage> {
  final TransformationController _tc = TransformationController();

  @override
  void dispose() {
    _tc.dispose();
    super.dispose();
  }

  void _toggleZoom() {
    final m = _tc.value;
    final isZoomed = m.storage[0] > 1.01;
    _tc.value = isZoomed
        ? Matrix4.identity()
        : (Matrix4.identity()..scale(2.5));
  }

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: () => Navigator.pop(context),
      onDoubleTap: _toggleZoom,
      child: Material(
        color: Colors.black.withOpacity(0.95),
        child: Stack(
          children: [
            Center(
              child: Hero(
                tag: widget.heroTag,
                child: InteractiveViewer(
                  transformationController: _tc,
                  minScale: 1.0,
                  maxScale: 5.0,
                  child: Image.network(
                    widget.url,
                    fit: BoxFit.contain,
                    loadingBuilder: (ctx, child, progress) {
                      if (progress == null) return child;
                      final total = progress.expectedTotalBytes;
                      final loaded = progress.cumulativeBytesLoaded;
                      return SizedBox.expand(
                        child: Center(
                          child: CircularProgressIndicator(
                            value: total != null ? loaded / total : null,
                          ),
                        ),
                      );
                    },
                    errorBuilder: (_, __, ___) => const Icon(
                      Icons.broken_image_outlined,
                      color: Colors.white70,
                      size: 64,
                    ),
                  ),
                ),
              ),
            ),
            SafeArea(
              child: Align(
                alignment: Alignment.topRight,
                child: IconButton(
                  tooltip: 'Close',
                  icon: const Icon(Icons.close, color: Colors.white),
                  onPressed: () => Navigator.pop(context),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
