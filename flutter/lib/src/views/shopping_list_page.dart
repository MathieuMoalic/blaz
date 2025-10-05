// lib/src/views/shopping_list_page.dart
import 'package:flutter/material.dart';
import '../api.dart';

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
    setState(() {
      _future = f;
    });
    await f;
  }

  Future<void> _add() async {
    final t = _ctrl.text.trim();
    if (t.isEmpty) return;
    await createShoppingItem(t);
    _ctrl.clear();
    _refresh();
  }

  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        const _Title('Shopping list'),
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
                if (snap.hasError)
                  return Center(child: Text('Error: ${snap.error}'));
                final items = snap.data ?? const [];
                if (items.isEmpty) return const Center(child: Text('No items'));
                return ListView.separated(
                  itemCount: items.length,
                  padding: const EdgeInsets.all(12),
                  itemBuilder: (_, i) {
                    final it = items[i];
                    return CheckboxListTile(
                      value: it.done,
                      onChanged: (v) async {
                        await toggleShoppingItem(id: it.id, done: v ?? false);
                        _refresh();
                      },
                      title: Text(it.text),
                      secondary: IconButton(
                        icon: const Icon(Icons.delete_outline),
                        onPressed: () async {
                          await deleteShoppingItem(it.id);
                          _refresh();
                        },
                      ),
                    );
                  },
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

class _Title extends StatelessWidget {
  final String t;
  const _Title(this.t);
  @override
  Widget build(BuildContext context) {
    return Container(
      height: 48,
      alignment: Alignment.centerLeft,
      padding: const EdgeInsets.symmetric(horizontal: 12),
      child: Text(t, style: Theme.of(context).textTheme.titleLarge),
    );
  }
}
