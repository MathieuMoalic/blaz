import 'package:flutter/material.dart';
import '../api.dart';
import '../widgets/app_title.dart';

const kCategories = <String>[
  'Produce',
  'Dairy',
  'Bakery',
  'Meat & Fish',
  'Pantry',
  'Spices',
  'Frozen',
  'Beverages',
  'Household',
  'Other',
];

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
    setState(() => _future = f);
    await f;
    if (!mounted) return;
    setState(() => _hidden.clear());
  }

  /// Helper to apply an in-place update and immediately rebuild the list view.
  Future<void> _applyLocalUpdate(
    List<ShoppingItem> Function(List<ShoppingItem>) transform,
  ) async {
    // Ensure we have current data; if first load still pending, await it.
    final List<ShoppingItem> current;
    current = (_cache.isEmpty ? await _future : _cache);
    final updated = transform(List<ShoppingItem>.from(current));
    _cache = updated;
    setState(() => _future = Future.value(updated));
  }

  Future<void> _add() async {
    final t = _ctrl.text.trim();
    if (t.isEmpty) return;

    try {
      final created = await createShoppingItem(t);
      _ctrl.clear();

      // Show immediately.
      await _applyLocalUpdate((list) {
        list.add(created);
        return list;
      });
    } catch (_) {
      // Fallback to a refresh if something went wrong.
      _refresh();
    }
  }

  String _catLabel(String? c) => (c == null || c.trim().isEmpty) ? 'Other' : c;

  Map<String, List<ShoppingItem>> _group(List<ShoppingItem> items) {
    final map = <String, List<ShoppingItem>>{};
    for (final it in items) {
      final key = _catLabel(it.category);
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
                  decoration: const InputDecoration(
                    labelText: 'Add item',
                    border: OutlineInputBorder(),
                  ),
                  onSubmitted: (_) => _add(),
                ),
              ),
              const SizedBox(width: 8),
              FilledButton(onPressed: _add, child: const Text('Add')),
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
                  ...kCategories.where(grouped.containsKey),
                  ...grouped.keys.where((c) => !kCategories.contains(c)),
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
                              cat,
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
                                  await _applyLocalUpdate(
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
    String cat = _catLabel(it.category);

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
                              items: kCategories
                                  .map(
                                    (c) => DropdownMenuItem(
                                      value: c,
                                      child: Text(c),
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
                              await _applyLocalUpdate((list) {
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
            color: c.surface.withOpacity(0.65),
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
