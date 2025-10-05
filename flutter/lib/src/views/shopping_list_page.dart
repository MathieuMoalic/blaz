import 'package:flutter/material.dart';

class ShoppingListPage extends StatefulWidget {
  const ShoppingListPage({super.key});
  @override
  State<ShoppingListPage> createState() => _ShoppingListPageState();
}

class _ShoppingListPageState extends State<ShoppingListPage> {
  final _controller = TextEditingController();
  final List<_Item> _items = [];

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  void _addItem() {
    final text = _controller.text.trim();
    if (text.isEmpty) return;
    setState(() {
      _items.add(_Item(text));
      _controller.clear();
    });
  }

  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        const SizedBox(height: 12),
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 12),
          child: Row(
            children: [
              Expanded(
                child: TextField(
                  controller: _controller,
                  decoration: const InputDecoration(
                    labelText: 'Add item',
                    hintText: 'e.g. 1kg flour',
                  ),
                  onSubmitted: (_) => _addItem(),
                ),
              ),
              const SizedBox(width: 8),
              FilledButton.icon(
                onPressed: _addItem,
                icon: const Icon(Icons.add),
                label: const Text('Add'),
              ),
            ],
          ),
        ),
        const Divider(height: 24),
        Expanded(
          child: _items.isEmpty
              ? const Center(child: Text('Empty list'))
              : ListView.separated(
                  padding: const EdgeInsets.all(12),
                  itemCount: _items.length,
                  itemBuilder: (_, i) => CheckboxListTile(
                    value: _items[i].done,
                    onChanged: (v) =>
                        setState(() => _items[i].done = v ?? false),
                    title: Text(
                      _items[i].text,
                      style: _items[i].done
                          ? const TextStyle(
                              decoration: TextDecoration.lineThrough,
                            )
                          : null,
                    ),
                    secondary: IconButton(
                      icon: const Icon(Icons.delete_outline),
                      onPressed: () => setState(() => _items.removeAt(i)),
                    ),
                  ),
                  separatorBuilder: (_, __) => const Divider(height: 1),
                ),
        ),
      ],
    );
  }
}

class _Item {
  _Item(this.text);
  final String text;
  bool done = false;
}
