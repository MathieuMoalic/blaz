import 'package:flutter/material.dart';
import '../api.dart';
import 'dart:async';

final List<CategoryOption> kShoppingCategoryOptions = kCategoryOptions;

final Map<String, String> kShoppingCategoryLabelByValue = {
  for (final o in kShoppingCategoryOptions) o.value: o.label,
};

List<String> get kShoppingCategoryValues =>
    kShoppingCategoryOptions.map((o) => o.value).toList(growable: false);

/// Format shopping item text with sentence case (capitalize first word, keep units lowercase)
String _formatItemText(String text) {
  if (text.isEmpty) return text;
  
  // Common measurement units to keep lowercase
  const units = {'g', 'kg', 'ml', 'l', 'tsp', 'tbsp', 'oz', 'lb'};
  
  final words = text.toLowerCase().split(' ');
  if (words.isEmpty) return text;
  
  // Find first word that's not a number or unit, and capitalize it
  bool capitalized = false;
  final formatted = words.map((word) {
    if (word.isEmpty) return word;
    
    // If it's a unit or number, keep lowercase
    if (units.contains(word) || double.tryParse(word) != null || word.contains('-')) {
      return word;
    }
    
    // Capitalize first non-unit word
    if (!capitalized) {
      capitalized = true;
      return word[0].toUpperCase() + word.substring(1);
    }
    
    return word;
  }).join(' ');
  
  return formatted;
}

class ShoppingListPage extends StatefulWidget {
  const ShoppingListPage({super.key});
  @override
  State<ShoppingListPage> createState() => ShoppingListPageState();
}

// RENAMED to be accessible by GlobalKey
class ShoppingListPageState extends State<ShoppingListPage> {
  late Future<List<ShoppingItem>> _future;
  final _ctrl = TextEditingController();

  /// Local cache of items for instant UI updates.
  List<ShoppingItem> _cache = const <ShoppingItem>[];

  /// Hide rows immediately when checked; they vanish before the network round-trip.
  final Set<int> _hidden = <int>{};

  /// Track collapsed categories
  final Set<String> _collapsedCategories = <String>{};

  // NEW: soft loading flag
  bool _refreshing = false;

  @override
  void initState() {
    super.initState();
    _future = _loadInitial();
  }

  @override
  void dispose() {
    _ctrl.dispose();
    super.dispose();
  }

  Future<List<ShoppingItem>> _loadInitial() async {
    final list = await fetchShoppingList();
    _cache = list;
    return list;
  }

  Future<List<ShoppingItem>> _loadFromServer() async {
    final list = await fetchShoppingList();
    _cache = list;
    return list;
  }

  /// Public refresh callable from HomeShell.
  /// Does not blank the list.
  Future<void> refresh() async {
    if (_refreshing) return;
    setState(() => _refreshing = true);

    try {
      final list = await _loadFromServer();
      if (!mounted) return;
      setState(() {
        _hidden.clear();
        _future = Future<List<ShoppingItem>>.value(list);
      });
    } finally {
      if (mounted) setState(() => _refreshing = false);
    }
  }

  Future<void> _refreshPull() async {
    await refresh();
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

    _applyLocalUpdate((list) => [tempItem, ...list]);

    try {
      final created = await createShoppingItem(raw);
      if (!mounted) return;

      _applyLocalUpdate((list) {
        // Remove the temp item
        list = list.where((x) => x.id != tempId).toList();
        
        // Check if an item with the same ID already exists (merge case)
        final existingIdx = list.indexWhere((x) => x.id == created.id);
        if (existingIdx != -1) {
          // Replace the existing item with the merged one
          list[existingIdx] = created;
        } else {
          // Add as new item
          list.insert(0, created);
        }
        return list;
      });

      // Immediate refresh to sync with server state
      unawaited(refresh());
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
        Expanded(
          child: Stack(
            children: [
              RefreshIndicator(
                onRefresh: _refreshPull,
                child: FutureBuilder<List<ShoppingItem>>(
                  future: _future,
                  builder: (context, snap) {
                    // Use cache while loading to avoid flicker.
                    final all =
                        (snap.connectionState == ConnectionState.done &&
                            snap.data != null)
                        ? snap.data!
                        : _cache;

                    if (all.isEmpty) {
                      if (snap.hasError) {
                        return Center(child: Text('Error: ${snap.error}'));
                      }
                      if (snap.connectionState != ConnectionState.done) {
                        return const Center(child: CircularProgressIndicator());
                      }
                      return const Center(child: Text('No items'));
                    }

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
                      padding: const EdgeInsets.fromLTRB(12, 8, 12, 12),
                      itemBuilder: (context, section) {
                        final cat = orderedCats[section];
                        final rows = grouped[cat]!;
                        final isCollapsed = _collapsedCategories.contains(cat);
                        
                        return Card(
                          margin: EdgeInsets.zero,
                          clipBehavior: Clip.antiAlias,
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.stretch,
                            children: [
                              InkWell(
                                onTap: () {
                                  setState(() {
                                    if (isCollapsed) {
                                      _collapsedCategories.remove(cat);
                                    } else {
                                      _collapsedCategories.add(cat);
                                    }
                                  });
                                },
                                child: Container(
                                  color:
                                      theme.colorScheme.surfaceContainerHighest,
                                  padding: const EdgeInsets.symmetric(
                                    horizontal: 12,
                                    vertical: 6,
                                  ),
                                  child: Row(
                                    children: [
                                      Icon(
                                        isCollapsed
                                            ? Icons.chevron_right
                                            : Icons.expand_more,
                                        size: 20,
                                      ),
                                      const SizedBox(width: 8),
                                      Expanded(
                                        child: Text(
                                          kShoppingCategoryLabelByValue[cat] ?? cat,
                                          style: theme.textTheme.titleMedium,
                                        ),
                                      ),
                                    ],
                                  ),
                                ),
                              ),
                              if (!isCollapsed)
                                for (final it in rows)
                                  _RowTile(
                                    item: it,
                                    onChanged: (v) async {
                                      setState(() => _hidden.add(it.id));
                                      try {
                                        final updated = await toggleShoppingItem(
                                          id: it.id,
                                          done: v ?? false,
                                        );
                                        _applyLocalUpdate(
                                          (list) => list
                                              .where((x) => x.id != updated.id)
                                              .toList(),
                                        );
                                      } finally {
                                        // Keep state consistent; still no flicker.
                                        await refresh();
                                      }
                                    },
                                    onEdit: () => _editItem(context, it),
                                  ),
                            ],
                          ),
                        );
                      },
                      separatorBuilder: (_, __) => const SizedBox(height: 4),
                      itemCount: orderedCats.length,
                    );
                  },
                ),
              ),

              // Subtle progress bar during refresh.
              if (_refreshing)
                const Positioned(
                  left: 0,
                  right: 0,
                  top: 0,
                  child: LinearProgressIndicator(minHeight: 2),
                ),

              // Floating "Add item" button in lower right
              Positioned(
                right: 16,
                bottom: 16,
                child: FloatingActionButton(
                  onPressed: _showAddItemDialog,
                  tooltip: 'Add item',
                  child: const Icon(Icons.add),
                ),
              ),
            ],
          ),
        ),
      ],
    );
  }

  Future<void> _showAddItemDialog() async {
    _ctrl.clear();
    
    // Get all unique item texts from cache for suggestions
    final allItemTexts = _cache.map((item) => item.text).toSet().toList();
    
    await showDialog(
      context: context,
      builder: (ctx) {
        return _AddItemDialog(
          controller: _ctrl,
          allItemTexts: allItemTexts,
          onAdd: (text) {
            Navigator.pop(ctx);
            _add(text);
          },
        );
      },
    );
  }

  // _editItem unchanged except replace any _refresh() calls with refresh()
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
                              Navigator.pop(ctx, true);

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

    if (changed == true) {
      setState(() {});
      // Optionally pull a fresh server snapshot
      unawaited(refresh());
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
            visualDensity: VisualDensity.compact,
            minVerticalPadding: 0,
            contentPadding: const EdgeInsets.symmetric(horizontal: 8, vertical: 0),
            onTap: onEdit,
            leading: Checkbox(
              value: false, // only active items are shown
              onChanged: onChanged,
              visualDensity: VisualDensity.compact,
              materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
            ),
            title: Text(
              _formatItemText(item.text),
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

/// Stateful dialog for adding items with fuzzy search suggestions
class _AddItemDialog extends StatefulWidget {
  final TextEditingController controller;
  final List<String> allItemTexts;
  final ValueChanged<String> onAdd;

  const _AddItemDialog({
    required this.controller,
    required this.allItemTexts,
    required this.onAdd,
  });

  @override
  State<_AddItemDialog> createState() => _AddItemDialogState();
}

class _AddItemDialogState extends State<_AddItemDialog> {
  List<String> _suggestions = [];
  
  @override
  void initState() {
    super.initState();
    widget.controller.addListener(_onTextChanged);
  }
  
  @override
  void dispose() {
    widget.controller.removeListener(_onTextChanged);
    super.dispose();
  }
  
  void _onTextChanged() {
    if (!mounted) return;
    
    try {
      final query = widget.controller.text.trim().toLowerCase();
      print('Query changed: "$query"');
      
      if (query.isEmpty) {
        setState(() => _suggestions = []);
        return;
      }
      
      // Fuzzy match and score all items
      final scored = <({String text, double score})>[];
      for (final text in widget.allItemTexts) {
        final score = fuzzyScore(text, query);
        if (score > 0) {
          scored.add((text: text, score: score));
        }
      }
      
      scored.sort((a, b) => b.score.compareTo(a.score));
      final newSuggestions = scored.take(5).map((item) => item.text).toList();
      
      print('Found ${newSuggestions.length} suggestions');
      
      if (mounted) {
        setState(() => _suggestions = newSuggestions);
      }
    } catch (e, stack) {
      // If there's an error, just clear suggestions
      print('Error in fuzzy search: $e\n$stack');
      if (mounted) {
        setState(() => _suggestions = []);
      }
    }
  }
  
  double fuzzyScore(String text, String query) {
    final textLower = text.toLowerCase();
    
    // Exact match
    if (textLower == query) return 1000;
    
    // Starts with
    if (textLower.startsWith(query)) return 900;
    
    // Word boundary match
    final words = textLower.split(' ');
    for (final word in words) {
      if (word.startsWith(query)) return 800;
    }
    
    // Contains substring
    if (textLower.contains(query)) return 700;
    
    // Fuzzy character-by-character match
    return fuzzyCharMatch(textLower, query);
  }
  
  double fuzzyCharMatch(String text, String pattern) {
    int textIdx = 0;
    int patternIdx = 0;
    int matchCount = 0;
    int consecutiveMatches = 0;
    double score = 0;
    
    while (textIdx < text.length && patternIdx < pattern.length) {
      if (text[textIdx] == pattern[patternIdx]) {
        matchCount++;
        consecutiveMatches++;
        score += 10 + consecutiveMatches;
        patternIdx++;
      } else {
        consecutiveMatches = 0;
      }
      textIdx++;
    }
    
    if (patternIdx != pattern.length) return 0;
    
    final matchRatio = matchCount / pattern.length;
    final lengthRatio = pattern.length / text.length;
    return score * matchRatio * lengthRatio;
  }
  
  void _submitText() {
    final text = widget.controller.text.trim();
    if (text.isNotEmpty) {
      widget.onAdd(text);
    }
  }
  
  @override
  Widget build(BuildContext context) {
    print('Building dialog, suggestions count: ${_suggestions.length}');
    return AlertDialog(
      title: const Text('Add item'),
      content: SizedBox(
        width: double.maxFinite,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            TextField(
              controller: widget.controller,
              autofocus: true,
              textInputAction: TextInputAction.done,
              decoration: const InputDecoration(
                hintText: 'e.g. 2 kg apples',
                border: OutlineInputBorder(),
              ),
              onSubmitted: (v) => _submitText(),
            ),
            // Fixed height container for suggestions to prevent dialog resizing
            if (_suggestions.isNotEmpty) ...[
              const SizedBox(height: 12),
              const Text(
                'Suggestions:',
                style: TextStyle(fontSize: 12, fontWeight: FontWeight.bold),
              ),
              const SizedBox(height: 4),
              SizedBox(
                height: 240,
                child: ListView.builder(
                  itemCount: _suggestions.length,
                  itemBuilder: (context, index) {
                    final suggestion = _suggestions[index];
                    return ListTile(
                      dense: true,
                      visualDensity: VisualDensity.compact,
                      contentPadding: const EdgeInsets.symmetric(horizontal: 8),
                      title: Text(_formatItemText(suggestion)),
                      trailing: const Icon(Icons.arrow_forward, size: 16),
                      onTap: () {
                        widget.controller.text = suggestion;
                        _submitText();
                      },
                    );
                  },
                ),
              ),
            ],
          ],
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: const Text('Cancel'),
        ),
        FilledButton(
          onPressed: _submitText,
          child: const Text('Add'),
        ),
      ],
    );
  }
}
