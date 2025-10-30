// lib/src/views/shopping_list_page.dart

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

  @override
  void initState() {
    super.initState();
    _future = fetchShoppingList();
  }

  @override
  void dispose() {
    _ctrl.dispose();
    super.dispose();
  }

  Future<void> _refresh() async {
    final f = fetchShoppingList();
    setState(() => _future = f);
    await f;
  }

  Future<void> _add() async {
    final t = _ctrl.text.trim();
    if (t.isEmpty) return;
    await createShoppingItem(t); // backend will auto-guess category
    _ctrl.clear();
    _refresh();
  }

  String _catLabel(String? c) => (c == null || c.trim().isEmpty) ? 'Other' : c;

  Map<String, List<ShoppingItem>> _group(List<ShoppingItem> items) {
    final map = <String, List<ShoppingItem>>{};
    for (final it in items) {
      final key = _catLabel(it.category);
      map.putIfAbsent(key, () => []).add(it);
    }
    // sort items by id inside each category (stable with your ORDER BY id)
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
        const AppTitle('Shopping list'),
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
                final items = snap.data ?? const <ShoppingItem>[];
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
                          Container(
                            color: theme.colorScheme.surfaceVariant.withOpacity(
                              0.5,
                            ),
                            padding: const EdgeInsets.fromLTRB(12, 8, 12, 6),
                            child: Text(
                              cat,
                              style: theme.textTheme.titleMedium,
                            ),
                          ),
                          ...rows.map((it) {
                            return Column(
                              children: [
                                ListTile(
                                  leading: Checkbox(
                                    value: it.done,
                                    onChanged: (v) async {
                                      await toggleShoppingItem(
                                        id: it.id,
                                        done: v ?? false,
                                      );
                                      _refresh();
                                    },
                                  ),
                                  title: Text(
                                    it.text,
                                    style: it.done
                                        ? const TextStyle(
                                            decoration:
                                                TextDecoration.lineThrough,
                                          )
                                        : null,
                                  ),
                                  trailing: Row(
                                    mainAxisSize: MainAxisSize.min,
                                    children: [
                                      // Category dropdown
                                      DropdownButton<String>(
                                        value: _catLabel(it.category),
                                        underline: const SizedBox(),
                                        onChanged: (value) async {
                                          if (value == null) return;
                                          final newCat = value == 'Other'
                                              ? ''
                                              : value;
                                          await updateShoppingItem(
                                            id: it.id,
                                            category: newCat,
                                          );
                                          _refresh();
                                        },
                                        items: kCategories
                                            .map(
                                              (c) => DropdownMenuItem(
                                                value: c,
                                                child: Text(c),
                                              ),
                                            )
                                            .toList(),
                                      ),
                                      IconButton(
                                        tooltip: 'Delete',
                                        icon: const Icon(Icons.delete_outline),
                                        onPressed: () async {
                                          await deleteShoppingItem(it.id);
                                          _refresh();
                                        },
                                      ),
                                    ],
                                  ),
                                ),
                                const Divider(height: 1),
                              ],
                            );
                          }),
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
}
