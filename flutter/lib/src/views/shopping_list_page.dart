import 'package:flutter/material.dart';
import '../api.dart';
import 'dart:async';

final List<CategoryOption> kShoppingCategoryOptions = kCategoryOptions;

final Map<String, String> kShoppingCategoryLabelByValue = {
  for (final o in kShoppingCategoryOptions) o.value: o.label,
};

List<String> get kShoppingCategoryValues =>
    kShoppingCategoryOptions.map((o) => o.value).toList(growable: false);

class ShoppingListPage extends StatefulWidget {
  const ShoppingListPage({super.key});
  @override
  State<ShoppingListPage> createState() => _ShoppingListPageState();
}

class _ShoppingListPageState extends State<ShoppingListPage> {
  late Future<List<ShoppingItem>> _future;
  final _ctrl = TextEditingController();

  /// Local cache of items for instant UI updates.
  List<ShoppingItem> _cache = const <ShoppingItem>[];

  /// Hide rows immediately when checked; they vanish before the network round-trip.
  final Set<int> _hidden = <int>{};

  @override
  void initState() {
    super.initState();
    _future = _load();
  }

  @override
  void dispose() {
    _ctrl.dispose();
    super.dispose();
  }

  Future<List<ShoppingItem>> _load() async {
    final list = await fetchShoppingList();
    _cache = list;
    return list;
  }

  Future<void> _refresh() async {
    final f = _load();
    setState(() {
      _future = f;
    });
    await f;
    if (!mounted) return;
    setState(() {
      _hidden.clear();
    });
  }

  void _applyLocalUpdate(
    List<ShoppingItem> Function(List<ShoppingItem>) transform,
  ) {
    final updated = transform(List<ShoppingItem>.from(_cache));
    _cache = updated;
    setState(() {
      _future = Future<List<ShoppingItem>>.value(updated);
    });
  }

  Future<void> _add([String? initial]) async {
    final raw = (initial ?? _ctrl.text).trim();
    if (raw.isEmpty) return;

    _ctrl.clear();

    final tempId = -DateTime.now().microsecondsSinceEpoch;
    final tempItem = ShoppingItem(
      id: tempId,
      text: raw,
      done: false,
      category: null,
    );

    // Show instantly
    _applyLocalUpdate((list) => [tempItem, ...list]);

    try {
      final created = await createShoppingItem(raw);

      if (!mounted) return;

      _applyLocalUpdate((list) {
        final idx = list.indexWhere((x) => x.id == tempId);
        if (idx != -1) {
          list[idx] = created;
        } else {
          list.insert(0, created);
        }
        return list;
      });

      // If your backend assigns category asynchronously,
      // this picks up the final category without blocking UI.
      unawaited(Future.delayed(const Duration(milliseconds: 800), _refresh));
    } catch (e) {
      _applyLocalUpdate((list) => list.where((x) => x.id != tempId).toList());
      if (mounted) {
        ScaffoldMessenger.of(
          context,
        ).showSnackBar(SnackBar(content: Text('Could not add item: $e')));
      }
    }
  }

  String _catValue(String? c) =>
      (c == null || c.trim().isEmpty) ? 'Other' : c.trim();

  Map<String, List<ShoppingItem>> _group(List<ShoppingItem> items) {
    final map = <String, List<ShoppingItem>>{};
    for (final it in items) {
      final key = _catValue(it.category);
      map.putIfAbsent(key, () => []).add(it);
    }
    for (final v in map.values) {
      v.sort((a, b) => a.id.compareTo(b.id));
    }
    return map;
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);

    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.all(12),
          child: Row(
            children: [
              Expanded(
                child: TextField(
                  controller: _ctrl,
                  textInputAction: TextInputAction.done,
                  onSubmitted: (v) => _add(v),
                  decoration: const InputDecoration(
                    labelText: 'Add item',
                    border: OutlineInputBorder(),
                  ),
                ),
              ),
              const SizedBox(width: 8),
              FilledButton(
                onPressed: () => _add(_ctrl.text),
                child: const Text('Add'),
              ),
            ],
          ),
        ),
        Expanded(
          child: RefreshIndicator(
            onRefresh: _refresh,
            child: FutureBuilder<List<ShoppingItem>>(
              future: _future,
              builder: (context, snap) {
                if (snap.connectionState != ConnectionState.done) {
                  return const Center(child: CircularProgressIndicator());
                }
                if (snap.hasError) {
                  return Center(child: Text('Error: ${snap.error}'));
                }
                final all = (snap.data ?? const <ShoppingItem>[]);
                // Only show active, non-hidden items.
                final items = all
                    .where((i) => !i.done && !_hidden.contains(i.id))
                    .toList();

                if (items.isEmpty) {
                  return const Center(child: Text('No items'));
                }

                final grouped = _group(items);
                final orderedCats = <String>[
                  ...kShoppingCategoryValues.where(grouped.containsKey),
                  ...grouped.keys.where(
                    (c) => !kShoppingCategoryValues.contains(c),
                  ),
                ];

                return ListView.separated(
                  padding: const EdgeInsets.fromLTRB(12, 0, 12, 12),
                  itemBuilder: (context, section) {
                    final cat = orderedCats[section];
                    final rows = grouped[cat]!;
                    return Card(
                      clipBehavior: Clip.antiAlias,
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.stretch,
                        children: [
                          // Section header (category printed once)
                          Container(
                            color: theme.colorScheme.surfaceContainerHighest,
                            padding: const EdgeInsets.fromLTRB(12, 8, 12, 6),
                            child: Text(
                              kShoppingCategoryLabelByValue[cat] ?? cat,
                              style: theme.textTheme.titleMedium,
                            ),
                          ),
                          for (final it in rows)
                            _RowTile(
                              item: it,
                              onChanged: (v) async {
                                // Optimistically hide immediately.
                                setState(() => _hidden.add(it.id));
                                try {
                                  final updated = await toggleShoppingItem(
                                    id: it.id,
                                    done: v ?? false,
                                  );
                                  // Remove it from local cache immediately too.
                                  _applyLocalUpdate(
                                    (list) => list
                                        .where((x) => x.id != updated.id)
                                        .toList(),
                                  );
                                } finally {
                                  // Ensure server state and local state stay in sync.
                                  _refresh();
                                }
                              },
                              onEdit: () => _editItem(context, it),
                            ),
                        ],
                      ),
                    );
                  },
                  separatorBuilder: (_, __) => const SizedBox(height: 8),
                  itemCount: orderedCats.length,
                );
              },
            ),
          ),
        ),
      ],
    );
  }

  Future<void> _editItem(BuildContext context, ShoppingItem it) async {
    final ctrl = TextEditingController(text: it.text);
    String cat = _catValue(it.category);

    final changed = await showModalBottomSheet<bool>(
      context: context,
      showDragHandle: true,
      isScrollControlled: true,
      builder: (ctx) {
        final bottom = MediaQuery.of(ctx).viewInsets.bottom;
        return StatefulBuilder(
          builder: (ctx, setSheetState) {
            return Padding(
              padding: EdgeInsets.only(bottom: bottom),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Padding(
                    padding: const EdgeInsets.fromLTRB(16, 10, 16, 6),
                    child: Row(
                      children: [
                        Text(
                          'Edit item',
                          style: Theme.of(ctx).textTheme.titleMedium,
                        ),
                      ],
                    ),
                  ),
                  const Divider(height: 1),
                  Padding(
                    padding: const EdgeInsets.all(16),
                    child: Column(
                      children: [
                        TextField(
                          controller: ctrl,
                          decoration: const InputDecoration(
                            labelText: 'Item',
                            hintText: 'e.g. 2 lemons',
                            border: OutlineInputBorder(),
                          ),
                          textInputAction: TextInputAction.done,
                        ),
                        const SizedBox(height: 12),
                        Row(
                          children: [
                            const Text('Category'),
                            const SizedBox(width: 12),
                            DropdownButton<String>(
                              value: cat,
                              onChanged: (v) =>
                                  setSheetState(() => cat = v ?? 'Other'),
                              items: kShoppingCategoryOptions
                                  .map(
                                    (o) => DropdownMenuItem(
                                      value: o.value,
                                      child: Text(o.label),
                                    ),
                                  )
                                  .toList(),
                            ),
                            const Spacer(),
                            IconButton(
                              tooltip: 'Delete item',
                              onPressed: () async {
                                await deleteShoppingItem(it.id);
                                if (!context.mounted) return;
                                Navigator.pop(ctx, true);
                              },
                              icon: const Icon(Icons.delete_outline),
                            ),
                          ],
                        ),
                      ],
                    ),
                  ),
                  const Divider(height: 1),
                  Padding(
                    padding: const EdgeInsets.fromLTRB(16, 10, 16, 16),
                    child: Row(
                      children: [
                        Expanded(
                          child: OutlinedButton(
                            onPressed: () => Navigator.pop(ctx, false),
                            child: const Text('Cancel'),
                          ),
                        ),
                        const SizedBox(width: 12),
                        Expanded(
                          child: FilledButton(
                            onPressed: () async {
                              final newText = ctrl.text.trim();
                              final newCat = cat == 'Other' ? '' : cat;

                              final updated = await updateShoppingItem(
                                id: it.id,
                                text: newText.isEmpty ? it.text : newText,
                                category: newCat,
                              );
                              if (!context.mounted) return;
                              // Close first so the sheet feels instant…
                              Navigator.pop(ctx, true);
                              // …then reflect the change locally right away.
                              _applyLocalUpdate((list) {
                                final idx = list.indexWhere(
                                  (x) => x.id == it.id,
                                );
                                if (idx != -1) list[idx] = updated;
                                return list;
                              });
                            },
                            child: const Text('Save'),
                          ),
                        ),
                      ],
                    ),
                  ),
                ],
              ),
            );
          },
        );
      },
    );

    // If deleted or saved via the sheet, refresh grouping; otherwise do nothing.
    if (changed == true) {
      // Grouping may change when category changes; ensure fresh order from cache.
      setState(() {}); // triggers rebuild using already-updated _cache
    }
  }
}

/// A compact, translucent row that exposes edit on tap only.
class _RowTile extends StatelessWidget {
  final ShoppingItem item;
  final ValueChanged<bool?> onChanged;
  final VoidCallback onEdit;

  const _RowTile({
    required this.item,
    required this.onChanged,
    required this.onEdit,
  });

  @override
  Widget build(BuildContext context) {
    final c = Theme.of(context).colorScheme;
    return Column(
      children: [
        Container(
          decoration: BoxDecoration(
            // Slight translucency so the global background peeks through.
            color: c.surface.withValues(alpha: 0.65),
          ),
          child: ListTile(
            dense: true,
            minVerticalPadding: 6,
            contentPadding: const EdgeInsets.symmetric(horizontal: 8),
            onTap: onEdit,
            leading: Checkbox(
              value: false, // only active items are shown
              onChanged: onChanged,
              visualDensity: VisualDensity.compact,
              materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
            ),
            title: Text(
              item.text,
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
            ),
          ),
        ),
        const Divider(height: 1),
      ],
    );
  }
}
