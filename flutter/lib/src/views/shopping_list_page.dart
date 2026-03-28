import 'package:flutter/material.dart';
import 'package:shared_preferences/shared_preferences.dart';
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

String _catValue(String? c) =>
    (c == null || c.trim().isEmpty) ? 'Other' : c.trim();

/// Format date from "Recipe Title (2026-02-20)" format
/// Returns formatted like "Recipe Title (Today)", "Recipe Title (in 2 days)", "Recipe Title (Feb 20)"
String _formatRecipeWithDate(String recipeWithDate) {
  final match = RegExp(r'^(.+?)\s*\((\d{4}-\d{2}-\d{2})\)$').firstMatch(recipeWithDate);
  if (match == null) return recipeWithDate; // No date, return as-is
  
  final title = match.group(1)!;
  final dateStr = match.group(2)!; // e.g., "2026-02-20"
  
  try {
    final date = DateTime.parse(dateStr);
    final now = DateTime.now();
    final today = DateTime(now.year, now.month, now.day);
    final targetDay = DateTime(date.year, date.month, date.day);
    final diff = targetDay.difference(today).inDays;
    
    String dateLabel;
    if (diff == 0) {
      dateLabel = 'Today';
    } else if (diff == 1) {
      dateLabel = 'Tomorrow';
    } else if (diff == -1) {
      dateLabel = '1 day ago';
    } else if (diff > 1 && diff <= 7) {
      dateLabel = 'in $diff days';
    } else if (diff < -1 && diff >= -7) {
      dateLabel = '${diff.abs()} days ago';
    } else {
      // Show month + day for dates further out
      const months = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 
                      'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
      dateLabel = '${months[date.month - 1]} ${date.day}';
    }
    
    return '$title ($dateLabel)';
  } catch (_) {
    return recipeWithDate; // Parse error, return original
  }
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
  Set<String> _collapsedCategories = <String>{};

  /// User-defined category display order (persisted).
  List<String> _categoryOrder = kShoppingCategoryValues;

  // NEW: soft loading flag
  bool _refreshing = false;

  @override
  void initState() {
    super.initState();
    _loadPrefs();
    _future = _loadInitial();
  }

  @override
  void dispose() {
    _ctrl.dispose();
    super.dispose();
  }

  Future<void> _loadPrefs() async {
    final prefs = await SharedPreferences.getInstance();
    final collapsed = prefs.getStringList('shopping_collapsed_categories') ?? [];
    final savedOrder = prefs.getStringList('shopping_category_order');
    setState(() {
      _collapsedCategories = collapsed.toSet();
      if (savedOrder != null && savedOrder.isNotEmpty) {
        // Merge: keep saved order, append any new categories not yet in it.
        final known = savedOrder.toSet();
        _categoryOrder = [
          ...savedOrder,
          ...kShoppingCategoryValues.where((c) => !known.contains(c)),
        ];
      }
    });
  }

  Future<void> _saveCollapsedState() async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.setStringList('shopping_collapsed_categories', _collapsedCategories.toList());
  }

  Future<void> _saveCategoryOrder() async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.setStringList('shopping_category_order', _categoryOrder);
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
                      ..._categoryOrder.where(grouped.containsKey),
                      ...grouped.keys.where(
                        (c) => !_categoryOrder.contains(c),
                      ),
                    ];

                    return ReorderableListView.builder(
                      padding: const EdgeInsets.fromLTRB(12, 8, 12, 12),
                      buildDefaultDragHandles: false,
                      onReorder: (oldIndex, newIndex) {
                        setState(() {
                          // Reorder within the full _categoryOrder list by
                          // mapping visible positions back to their values.
                          final moved = orderedCats[oldIndex];
                          final target = newIndex > oldIndex
                              ? orderedCats[newIndex - 1]
                              : orderedCats[newIndex];
                          final order = List<String>.from(_categoryOrder);
                          order.remove(moved);
                          final insertAt = order.indexOf(target);
                          order.insert(
                            newIndex > oldIndex ? insertAt + 1 : insertAt,
                            moved,
                          );
                          _categoryOrder = order;
                        });
                        _saveCategoryOrder();
                      },
                      itemBuilder: (context, section) {
                        final cat = orderedCats[section];
                        final rows = grouped[cat]!;
                        final isCollapsed = _collapsedCategories.contains(cat);

                        return Padding(
                          key: ValueKey(cat),
                          padding: const EdgeInsets.only(bottom: 4),
                          child: Card(
                            margin: EdgeInsets.zero,
                            clipBehavior: Clip.antiAlias,
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.stretch,
                              children: [
                                Container(
                                  color: theme.colorScheme.surfaceContainerHighest,
                                  child: Row(
                                    children: [
                                      // Drag handle — only this widget initiates drag.
                                      ReorderableDragStartListener(
                                        index: section,
                                        child: Padding(
                                          padding: const EdgeInsets.symmetric(
                                            horizontal: 8,
                                            vertical: 6,
                                          ),
                                          child: Icon(
                                            Icons.drag_handle,
                                            size: 20,
                                            color: theme.colorScheme.onSurfaceVariant,
                                          ),
                                        ),
                                      ),
                                      // Tap-to-collapse fills the rest of the header.
                                      Expanded(
                                        child: InkWell(
                                          onTap: () {
                                            setState(() {
                                              if (isCollapsed) {
                                                _collapsedCategories.remove(cat);
                                              } else {
                                                _collapsedCategories.add(cat);
                                              }
                                            });
                                            _saveCollapsedState();
                                          },
                                          child: Padding(
                                            padding: const EdgeInsets.symmetric(
                                              horizontal: 4,
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
                                      ),
                                    ],
                                  ),
                                ),
                                if (!isCollapsed)
                                  for (var i = 0; i < rows.length; i++)
                                    _RowTile(
                                      item: rows[i],
                                      isLastInCategory: i == rows.length - 1,
                                      onChanged: (v) async {
                                        setState(() => _hidden.add(rows[i].id));
                                        try {
                                          final updated = await toggleShoppingItem(
                                            id: rows[i].id,
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
                                      onEdit: () => _editItem(context, rows[i]),
                                    ),
                              ],
                            ),
                          ),
                        );
                      },
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
    
    // Get all unique item texts including done items for suggestions
    final allItemTexts = await fetchAllShoppingTexts();
    
    if (!mounted) return;
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
    final result = await showModalBottomSheet<ShoppingItem>(
      context: context,
      showDragHandle: true,
      isScrollControlled: true,
      builder: (ctx) => _EditShoppingItemSheet(item: it),
    );

    if (result != null) {
      _applyLocalUpdate((list) {
        final idx = list.indexWhere((x) => x.id == it.id);
        if (idx != -1) list[idx] = result;
        return list;
      });
      unawaited(refresh());
    }
  }
}

/// Modal bottom sheet for editing a shopping list item.
/// Owns its own TextEditingControllers to avoid disposed-controller crashes.
class _EditShoppingItemSheet extends StatefulWidget {
  final ShoppingItem item;
  const _EditShoppingItemSheet({required this.item});

  @override
  State<_EditShoppingItemSheet> createState() => _EditShoppingItemSheetState();
}

class _EditShoppingItemSheetState extends State<_EditShoppingItemSheet> {
  late final TextEditingController _qtyCtrl;
  late final TextEditingController _unitCtrl;
  late final TextEditingController _nameCtrl;
  late final TextEditingController _notesCtrl;
  late String _cat;

  @override
  void initState() {
    super.initState();
    final parsed = parseIngredientLine(widget.item.text);
    _qtyCtrl = TextEditingController(
      text: parsed.quantity != null
          ? (parsed.quantity! % 1 == 0
              ? parsed.quantity!.toInt().toString()
              : parsed.quantity!.toString())
          : '',
    );
    _unitCtrl = TextEditingController(text: parsed.unit ?? '');
    _nameCtrl = TextEditingController(text: parsed.name);
    _notesCtrl = TextEditingController(text: widget.item.notes);
    _cat = _catValue(widget.item.category);
  }

  @override
  void dispose() {
    _qtyCtrl.dispose();
    _unitCtrl.dispose();
    _nameCtrl.dispose();
    _notesCtrl.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final bottom = MediaQuery.of(context).viewInsets.bottom;
    return Padding(
      padding: EdgeInsets.only(bottom: bottom),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 10, 16, 6),
            child: Row(
              children: [
                Text('Edit item', style: Theme.of(context).textTheme.titleMedium),
              ],
            ),
          ),
          const Divider(height: 1),
          Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              children: [
                Row(
                  children: [
                    Flexible(
                      flex: 2,
                      child: TextField(
                        controller: _qtyCtrl,
                        decoration: const InputDecoration(
                          labelText: 'Qty',
                          border: OutlineInputBorder(),
                          isDense: true,
                        ),
                        keyboardType: const TextInputType.numberWithOptions(decimal: true),
                        textInputAction: TextInputAction.next,
                      ),
                    ),
                    const SizedBox(width: 8),
                    Flexible(
                      flex: 2,
                      child: TextField(
                        controller: _unitCtrl,
                        decoration: const InputDecoration(
                          labelText: 'Unit',
                          hintText: 'g, ml…',
                          border: OutlineInputBorder(),
                          isDense: true,
                        ),
                        textInputAction: TextInputAction.next,
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: _nameCtrl,
                  autofocus: true,
                  decoration: const InputDecoration(
                    labelText: 'Name',
                    border: OutlineInputBorder(),
                    isDense: true,
                  ),
                  textInputAction: TextInputAction.next,
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: _notesCtrl,
                  decoration: const InputDecoration(
                    labelText: 'Notes',
                    hintText: 'e.g. ripe ones',
                    border: OutlineInputBorder(),
                    isDense: true,
                  ),
                  textInputAction: TextInputAction.done,
                ),
                const SizedBox(height: 12),
                Row(
                  children: [
                    const Text('Category'),
                    const SizedBox(width: 12),
                    DropdownButton<String>(
                      value: _cat,
                      onChanged: (v) => setState(() => _cat = v ?? 'Other'),
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
                        await deleteShoppingItem(widget.item.id);
                        if (!context.mounted) return;
                        Navigator.pop(context);
                      },
                      icon: const Icon(Icons.delete_outline),
                    ),
                  ],
                ),
                if (widget.item.recipeTitles != null &&
                    widget.item.recipeTitles!.isNotEmpty) ...[
                  const SizedBox(height: 16),
                  Container(
                    padding: const EdgeInsets.all(12),
                    decoration: BoxDecoration(
                      color: Colors.grey.withValues(alpha: 0.1),
                      borderRadius: BorderRadius.circular(8),
                    ),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        const Row(
                          children: [
                            Icon(Icons.restaurant_menu, size: 16),
                            SizedBox(width: 6),
                            Text(
                              'From recipes:',
                              style: TextStyle(fontWeight: FontWeight.w500),
                            ),
                          ],
                        ),
                        const SizedBox(height: 8),
                        ...widget.item.recipeTitles!
                            .split(', ')
                            .map((recipeWithDate) {
                          final formatted = _formatRecipeWithDate(recipeWithDate);
                          return Padding(
                            padding: const EdgeInsets.only(left: 22, bottom: 4),
                            child: Row(
                              children: [
                                const Text('• ', style: TextStyle(fontSize: 16)),
                                Expanded(
                                  child: Text(
                                    formatted,
                                    style: const TextStyle(fontSize: 14),
                                  ),
                                ),
                              ],
                            ),
                          );
                        }),
                      ],
                    ),
                  ),
                ],
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
                    onPressed: () => Navigator.pop(context),
                    child: const Text('Cancel'),
                  ),
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: FilledButton(
                    onPressed: () async {
                      final name = _nameCtrl.text.trim();
                      if (name.isEmpty) return;
                      final qty = double.tryParse(
                          _qtyCtrl.text.trim().replaceAll(',', '.'));
                      final unit = _unitCtrl.text.trim().isEmpty
                          ? null
                          : _unitCtrl.text.trim();
                      final newCat = _cat == 'Other' ? '' : _cat;

                      final updated = await updateShoppingItem(
                        id: widget.item.id,
                        name: name,
                        quantity: qty,
                        unit: unit,
                        category: newCat,
                        notes: _notesCtrl.text.trim(),
                      );
                      if (!context.mounted) return;
                      Navigator.pop(context, updated);
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
  }
}

/// A compact, translucent row that exposes edit on tap only.
class _RowTile extends StatelessWidget {
  final ShoppingItem item;
  final bool isLastInCategory;
  final ValueChanged<bool?> onChanged;
  final VoidCallback onEdit;

  const _RowTile({
    required this.item,
    required this.isLastInCategory,
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
            title: Row(
              children: [
                Expanded(
                  child: Text(
                    _formatItemText(item.text),
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
                if (item.notes.isNotEmpty) ...[
                  const SizedBox(width: 8),
                  Flexible(
                    child: Text(
                      item.notes,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                        fontSize: 12,
                        color: Theme.of(context).colorScheme.onSurface.withValues(alpha: 0.45),
                      ),
                    ),
                  ),
                ],
              ],
            ),
          ),
        ),
        if (!isLastInCategory) const Divider(height: 1),
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
      
      if (mounted) {
        setState(() => _suggestions = newSuggestions);
      }
    } catch (e) {
      // If there's an error, just clear suggestions
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
