import 'dart:async';
import 'package:flutter/material.dart';
import 'package:file_selector/file_selector.dart';
import '../api.dart';

class EditRecipePage extends StatefulWidget {
  final Recipe recipe;

  /// Optional index into `recipe.ingredients` to scroll to and highlight on
  /// open (used by the recipe detail "Review" action to jump straight to the
  /// first needs-review ingredient).
  final int? focusIngredientIndex;
  const EditRecipePage({
    super.key,
    required this.recipe,
    this.focusIngredientIndex,
  });

  @override
  State<EditRecipePage> createState() => _EditRecipePageState();
}

class _EditRecipePageState extends State<EditRecipePage> {
  final _form = GlobalKey<FormState>();

  late final TextEditingController _title;
  late final TextEditingController _source;
  late final TextEditingController _yieldText;
  late final TextEditingController _notes;

  late List<Ingredient> _ingredients;
  late List<String> _ingredientKeys;
  int _ingredientKeySeq = 0;
  late List<String> _instructions;
  late List<String> _instructionKeys;
  int _instructionKeySeq = 0;
  bool _busy = false;
  final _focusTileKey = GlobalKey();

  @override
  void initState() {
    super.initState();
    final r = widget.recipe;
    _title = TextEditingController(text: r.title);
    _source = TextEditingController(text: r.source);
    _yieldText = TextEditingController(text: r.yieldText);
    _notes = TextEditingController(text: r.notes);
    _ingredients = List.from(r.ingredients);
    _ingredientKeys = List.generate(
      _ingredients.length,
      (_) => _newIngredientKey(),
    );
    if (widget.focusIngredientIndex != null) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        final ctx = _focusTileKey.currentContext;
        if (ctx != null) {
          Scrollable.ensureVisible(
            ctx,
            duration: const Duration(milliseconds: 350),
            alignment: 0.15,
          );
        }
      });
    }
    _instructions = List.from(r.instructions);
    _instructionKeys = List.generate(
      _instructions.length,
      (_) => _newInstructionKey(),
    );
  }

  @override
  void dispose() {
    _title.dispose();
    _source.dispose();
    _yieldText.dispose();
    _notes.dispose();
    super.dispose();
  }

  String _newIngredientKey() => 'ingredient_${_ingredientKeySeq++}';
  String _newInstructionKey() => 'instruction_${_instructionKeySeq++}';

  Future<void> _save() async {
    if (!_form.currentState!.validate()) return;
    setState(() => _busy = true);
    try {
      await updateRecipe(
        id: widget.recipe.id,
        title: _title.text.trim(),
        source: _source.text.trim(),
        yieldText: _yieldText.text.trim(),
        notes: _notes.text.trim(),
        ingredients: _ingredients,
        instructions: _instructions,
      );
      if (mounted) Navigator.pop(context, true);
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(SnackBar(content: Text('Failed: $e')));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _changeImage() async {
    final typeGroup = const XTypeGroup(
      label: 'images',
      extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif'],
    );
    final file = await openFile(acceptedTypeGroups: [typeGroup]);
    if (file == null) return;

    setState(() => _busy = true);
    try {
      final bytes = await file.readAsBytes();
      await uploadRecipeImage(
        id: widget.recipe.id,
        filename: file.name,
        bytes: bytes,
      );
      if (!mounted) return;
      Navigator.pop(context, true);
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(SnackBar(content: Text('Image failed: $e')));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  /// Opens an edit dialog for the ingredient at [index], or -1 to add a new one.
  Future<void> _editIngredient(int index) async {
    final existing = index >= 0 ? _ingredients[index] : null;

    // Tapping a section header renames it.
    if (existing?.isSection == true) {
      await _renameSection(index);
      return;
    }

    final result = await showDialog<Ingredient>(
      context: context,
      builder: (ctx) => _IngredientDialog(
        title: index >= 0 ? 'Edit ingredient' : 'Add ingredient',
        initial: existing,
      ),
    );

    if (result != null) {
      setState(() {
        if (index >= 0) {
          _ingredients[index] = result;
        } else {
          _ingredients.add(result);
          _ingredientKeys.add(_newIngredientKey());
        }
      });
    }
  }

  void _removeIngredientAt(int index) {
    setState(() {
      _ingredients.removeAt(index);
      _ingredientKeys.removeAt(index);
    });
  }

  void _moveIngredient(int oldIndex, int newIndex) {
    if (newIndex > oldIndex) newIndex -= 1;
    setState(() {
      final ingredient = _ingredients.removeAt(oldIndex);
      final key = _ingredientKeys.removeAt(oldIndex);
      _ingredients.insert(newIndex, ingredient);
      _ingredientKeys.insert(newIndex, key);
    });
  }

  void _removeInstructionAt(int index) {
    setState(() {
      _instructions.removeAt(index);
      _instructionKeys.removeAt(index);
    });
  }

  void _moveInstruction(int oldIndex, int newIndex) {
    if (newIndex > oldIndex) newIndex -= 1;
    setState(() {
      final instruction = _instructions.removeAt(oldIndex);
      final key = _instructionKeys.removeAt(oldIndex);
      _instructions.insert(newIndex, instruction);
      _instructionKeys.insert(newIndex, key);
    });
  }

  Future<void> _editInstruction(int index) async {
    final existing = index >= 0 ? _instructions[index] : '';
    final result = await showDialog<List<String>>(
      context: context,
      builder: (ctx) => _InstructionDialog(
        title: index >= 0 ? 'Edit instruction' : 'Add instruction',
        initialText: existing,
      ),
    );

    if (result == null || result.isEmpty) return;

    setState(() {
      if (index >= 0) {
        _instructions.removeAt(index);
        _instructionKeys.removeAt(index);
        for (var i = 0; i < result.length; i++) {
          _instructions.insert(index + i, result[i]);
          _instructionKeys.insert(index + i, _newInstructionKey());
        }
      } else {
        for (final step in result) {
          _instructions.add(step);
          _instructionKeys.add(_newInstructionKey());
        }
      }
    });
  }

  Future<void> _renameSection(int index) async {
    final current = _ingredients[index].section ?? '';
    final ctrl = TextEditingController(text: current);
    final result = await showDialog<String>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Rename section'),
        content: TextField(
          controller: ctrl,
          autofocus: true,
          decoration: const InputDecoration(
            labelText: 'Section name',
            border: OutlineInputBorder(),
          ),
          onSubmitted: (v) {
            WidgetsBinding.instance.addPostFrameCallback((_) {
              if (ctx.mounted) Navigator.pop(ctx, v.trim());
            });
          },
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(ctx),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(ctx, ctrl.text.trim()),
            child: const Text('OK'),
          ),
        ],
      ),
    );
    ctrl.dispose();
    if (result != null && result.isNotEmpty) {
      setState(() => _ingredients[index] = Ingredient.sectionHeader(result));
    }
  }

  Future<void> _addSection() async {
    final ctrl = TextEditingController();
    final result = await showDialog<String>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Add section'),
        content: TextField(
          controller: ctrl,
          autofocus: true,
          decoration: const InputDecoration(
            labelText: 'Section name',
            hintText: 'e.g. Sauce, Topping…',
            border: OutlineInputBorder(),
          ),
          onSubmitted: (v) {
            WidgetsBinding.instance.addPostFrameCallback((_) {
              if (ctx.mounted) Navigator.pop(ctx, v.trim());
            });
          },
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(ctx),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(ctx, ctrl.text.trim()),
            child: const Text('Add'),
          ),
        ],
      ),
    );
    ctrl.dispose();
    if (result != null && result.isNotEmpty) {
      setState(() => _ingredients.add(Ingredient.sectionHeader(result)));
    }
  }

  Future<void> _reparseWithAi() async {
    setState(() => _busy = true);
    try {
      final parsed = await reparseIngredients(widget.recipe.id);
      if (!mounted) return;
      // Merge: keep any ingredients the LLM didn't return (by index)
      setState(() {
        for (var i = 0; i < parsed.length && i < _ingredients.length; i++) {
          _ingredients[i] = parsed[i];
        }
      });
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(SnackBar(content: Text('AI re-parse failed: $e')));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final gap = const SizedBox(height: 12);
    final theme = Theme.of(context);
    final needsReviewCount =
      _ingredients.where((i) => i.needsReview && !i.isSection).length;

    return Scaffold(
      appBar: AppBar(title: const Text('Edit recipe')),
      body: SafeArea(
        child: Form(
          key: _form,
          child: ListView(
            padding: const EdgeInsets.all(16),
            children: [
              // Title
              TextFormField(
                controller: _title,
                decoration: const InputDecoration(
                  labelText: 'Title *',
                  border: OutlineInputBorder(),
                ),
                textInputAction: TextInputAction.next,
                validator: (v) =>
                    (v == null || v.trim().isEmpty) ? 'Title required' : null,
              ),
              gap,

              // Image
              Card(
                child: Padding(
                  padding: const EdgeInsets.all(12),
                  child: Row(
                    children: [
                      const Icon(Icons.photo),
                      const SizedBox(width: 12),
                      const Expanded(child: Text('Recipe Image')),
                      FilledButton.icon(
                        onPressed: _busy ? null : _changeImage,
                        icon: const Icon(Icons.photo_outlined),
                        label: const Text('Change image'),
                      ),
                    ],
                  ),
                ),
              ),
              gap,

              // Ingredients
              Row(
                children: [
                  Text('Ingredients', style: theme.textTheme.titleSmall),
                  const Spacer(),
                  if (needsReviewCount > 0) ...[
                    Chip(
                      label: Text('$needsReviewCount need review'),
                      visualDensity: VisualDensity.compact,
                      materialTapTargetSize:
                          MaterialTapTargetSize.shrinkWrap,
                      backgroundColor: Theme.of(
                        context,
                      ).colorScheme.tertiaryContainer,
                      labelStyle: TextStyle(
                        fontSize: 12,
                        color: Theme.of(
                          context,
                        ).colorScheme.onTertiaryContainer,
                      ),
                    ),
                    const SizedBox(width: 4),
                  ],
                  PopupMenuButton<String>(
                    icon: const Icon(Icons.more_vert, size: 18),
                    tooltip: 'More',
                    onSelected: (v) {
                      if (v == 'reinterpret') _reparseWithAi();
                    },
                    itemBuilder: (_) => const [
                      PopupMenuItem(
                        value: 'reinterpret',
                        child: Text('Reinterpret ingredients'),
                      ),
                    ],
                  ),
                ],
              ),
              const SizedBox(height: 4),
              Card(
                child: Column(
                  children: [
                    ReorderableListView.builder(
                      shrinkWrap: true,
                      physics: const NeverScrollableScrollPhysics(),
                      buildDefaultDragHandles: false,
                      onReorder: (oldIndex, newIndex) {
                        if (_busy) return;
                        _moveIngredient(oldIndex, newIndex);
                      },
                      itemCount: _ingredients.length,
                      itemBuilder: (context, i) {
                        final isFocus =
                            widget.focusIngredientIndex != null &&
                                i == widget.focusIngredientIndex;
                        return _IngredientTile(
                          key: isFocus
                              ? _focusTileKey
                              : ValueKey(_ingredientKeys[i]),
                          index: i,
                          ingredient: _ingredients[i],
                          canReorder: !_busy,
                          onTap: _busy ? null : () => _editIngredient(i),
                          onDelete: _busy ? null : () => _removeIngredientAt(i),
                        );
                      },
                    ),
                    const Divider(height: 1),
                    ListTile(
                      leading: const Icon(Icons.add),
                      title: const Text('Add ingredient'),
                      onTap: _busy ? null : () => _editIngredient(-1),
                    ),
                    ListTile(
                      leading: const Icon(Icons.playlist_add),
                      title: const Text('Add section'),
                      onTap: _busy ? null : _addSection,
                    ),
                  ],
                ),
              ),
              gap,

              // Instructions
              Text('Instructions', style: theme.textTheme.titleSmall),
              const SizedBox(height: 4),
              Card(
                child: Column(
                  children: [
                    ReorderableListView.builder(
                      shrinkWrap: true,
                      physics: const NeverScrollableScrollPhysics(),
                      buildDefaultDragHandles: false,
                      onReorder: (oldIndex, newIndex) {
                        if (_busy) return;
                        _moveInstruction(oldIndex, newIndex);
                      },
                      itemCount: _instructions.length,
                      itemBuilder: (context, i) => _InstructionTile(
                        key: ValueKey(_instructionKeys[i]),
                        index: i,
                        text: _instructions[i],
                        canReorder: !_busy,
                        onTap: _busy ? null : () => _editInstruction(i),
                        onDelete: _busy ? null : () => _removeInstructionAt(i),
                      ),
                    ),
                    if (_instructions.isNotEmpty) const Divider(height: 1),
                    ListTile(
                      leading: const Icon(Icons.add),
                      title: const Text('Add step'),
                      onTap: _busy ? null : () => _editInstruction(-1),
                    ),
                  ],
                ),
              ),
              gap,

              // Notes
              TextField(
                controller: _notes,
                decoration: const InputDecoration(
                  labelText: 'Notes',
                  border: OutlineInputBorder(),
                  alignLabelWithHint: true,
                ),
                minLines: 2,
                maxLines: null,
              ),
              gap,

              // Source
              TextField(
                controller: _source,
                decoration: const InputDecoration(
                  labelText: 'Source',
                  border: OutlineInputBorder(),
                ),
                textInputAction: TextInputAction.next,
              ),
              gap,

              // Yield
              TextField(
                controller: _yieldText,
                decoration: const InputDecoration(
                  labelText: 'Yield',
                  border: OutlineInputBorder(),
                ),
                textInputAction: TextInputAction.next,
              ),
              const SizedBox(height: 16),
              FilledButton.icon(
                onPressed: _busy ? null : _save,
                icon: _busy
                    ? const SizedBox(
                        width: 16,
                        height: 16,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      )
                    : const Icon(Icons.check),
                label: const Text('Save'),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Helpers

String _fmtQty(double v) {
  final s = ((v * 100).round() / 100.0).toString();
  return s.endsWith('.0') ? s.replaceFirst('.0', '') : s;
}

// ---------------------------------------------------------------------------
// Ingredient tile

class _IngredientTile extends StatelessWidget {
  final int index;
  final Ingredient ingredient;
  final bool canReorder;
  final VoidCallback? onTap;
  final VoidCallback? onDelete;

  const _IngredientTile({
    super.key,
    required this.index,
    required this.ingredient,
    required this.canReorder,
    this.onTap,
    this.onDelete,
  });

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final muted = theme.colorScheme.onSurfaceVariant;

    // Section header row
    if (ingredient.isSection) {
      return ListTile(
        dense: true,
        contentPadding: const EdgeInsets.fromLTRB(16, 4, 4, 0),
        title: Text(
          ingredient.section!,
          style: theme.textTheme.labelLarge?.copyWith(
            color: theme.colorScheme.primary,
            letterSpacing: 0.5,
          ),
        ),
        trailing: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            IconButton(
              icon: const Icon(Icons.edit_outlined, size: 16),
              onPressed: onTap,
              visualDensity: VisualDensity.compact,
              tooltip: 'Rename section',
            ),
            IconButton(
              icon: const Icon(Icons.close, size: 16),
              onPressed: onDelete,
              visualDensity: VisualDensity.compact,
              tooltip: 'Remove section',
            ),
            canReorder
                ? ReorderableDragStartListener(
                    index: index,
                    child: const Padding(
                      padding: EdgeInsets.all(8),
                      child: Icon(Icons.drag_handle, size: 18),
                    ),
                  )
                : const Padding(
                    padding: EdgeInsets.all(8),
                    child: Icon(Icons.drag_handle, size: 18),
                  ),
          ],
        ),
      );
    }

    final isRaw = ingredient.raw;
    final needsReview = ingredient.needsReview && !ingredient.isSection;

    final qtyLabel = isRaw
        ? '?'
        : ingredient.quantity != null
        ? [
            _fmtQty(ingredient.quantity!),
            if (ingredient.unit != null) ingredient.unit!,
          ].join('\u00a0') // non-breaking space
        : '—';

    return ListTile(
      dense: true,
      tileColor: needsReview
          ? theme.colorScheme.error.withValues(alpha: 0.05)
          : null,
      leading: SizedBox(
        width: 52,
        child: Text(
          qtyLabel,
          style: theme.textTheme.bodySmall?.copyWith(
            color: muted,
            fontFeatures: const [FontFeature.tabularFigures()],
          ),
          textAlign: TextAlign.end,
        ),
      ),
      title: Text(
        ingredient.name,
        style: isRaw ? TextStyle(color: muted) : null,
      ),
      subtitle: needsReview
          ? Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Icon(
                  Icons.warning_amber_rounded,
                  size: 13,
                  color: theme.colorScheme.error,
                ),
                const SizedBox(width: 4),
                Text(
                  'Needs review',
                  style: theme.textTheme.labelSmall?.copyWith(
                    color: theme.colorScheme.error,
                    fontWeight: FontWeight.w600,
                  ),
                ),
                if (ingredient.prep != null && ingredient.prep!.isNotEmpty)
                  Expanded(
                    child: Text(
                      ' · ${ingredient.prep!}',
                      style: theme.textTheme.bodySmall?.copyWith(color: muted),
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
              ],
            )
          : (ingredient.prep != null && ingredient.prep!.isNotEmpty)
          ? Text(
              ingredient.prep!,
              style: theme.textTheme.bodySmall?.copyWith(color: muted),
            )
          : null,
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          IconButton(
            icon: const Icon(Icons.close, size: 18),
            onPressed: onDelete,
            visualDensity: VisualDensity.compact,
          ),
          canReorder
              ? ReorderableDragStartListener(
                  index: index,
                  child: const Padding(
                    padding: EdgeInsets.all(8),
                    child: Icon(Icons.drag_handle, size: 18),
                  ),
                )
              : const Padding(
                  padding: EdgeInsets.all(8),
                  child: Icon(Icons.drag_handle, size: 18),
                ),
        ],
      ),
      onTap: onTap,
    );
  }
}

// ---------------------------------------------------------------------------
// Instruction tile

class _InstructionTile extends StatelessWidget {
  final int index;
  final String text;
  final bool canReorder;
  final VoidCallback? onTap;
  final VoidCallback? onDelete;

  const _InstructionTile({
    super.key,
    required this.index,
    required this.text,
    required this.canReorder,
    this.onTap,
    this.onDelete,
  });

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);

    return ListTile(
      dense: true,
      leading: CircleAvatar(
        radius: 13,
        backgroundColor: theme.colorScheme.primaryContainer,
        foregroundColor: theme.colorScheme.onPrimaryContainer,
        child: Text(
          '${index + 1}',
          style: theme.textTheme.labelSmall?.copyWith(
            fontWeight: FontWeight.w700,
          ),
        ),
      ),
      title: Text(text),
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          IconButton(
            icon: const Icon(Icons.close, size: 18),
            onPressed: onDelete,
            visualDensity: VisualDensity.compact,
          ),
          canReorder
              ? ReorderableDragStartListener(
                  index: index,
                  child: const Padding(
                    padding: EdgeInsets.all(8),
                    child: Icon(Icons.drag_handle, size: 18),
                  ),
                )
              : const Padding(
                  padding: EdgeInsets.all(8),
                  child: Icon(Icons.drag_handle, size: 18),
                ),
        ],
      ),
      onTap: onTap,
    );
  }
}

// ---------------------------------------------------------------------------
// Ingredient edit dialog

class _IngredientDialog extends StatefulWidget {
  final String title;
  final Ingredient? initial;

  const _IngredientDialog({required this.title, this.initial});

  @override
  State<_IngredientDialog> createState() => _IngredientDialogState();
}

class _IngredientDialogState extends State<_IngredientDialog> {
  late final TextEditingController _qty;
  late final TextEditingController _unit;
  late final TextEditingController _name;
  late final TextEditingController _prep;
  int? _foodId;
  String? _foodName;

  @override
  void initState() {
    super.initState();
    final i = widget.initial;
    _qty = TextEditingController(
      text: i?.quantity != null ? _fmtQty(i!.quantity!) : '',
    );
    _unit = TextEditingController(text: i?.unit ?? '');
    _name = TextEditingController(text: i?.name ?? '');
    _prep = TextEditingController(text: i?.prep ?? '');
    _foodId = i?.foodId;
    _foodName = i?.canonicalName;
  }

  Future<void> _pickFood() async {
    final picked = await showModalBottomSheet<FoodResult>(
      context: context,
      showDragHandle: true,
      isScrollControlled: true,
      builder: (ctx) => const _FoodChooserSheet(),
    );
    if (picked == null || !mounted) return;
    // User correction teaches the system: lock this wording to the Food.
    final alias = (widget.initial?.name.trim() ?? '').isNotEmpty
        ? widget.initial!.name.trim()
        : _name.text.trim();
    if (alias.isNotEmpty) {
      try {
        await confirmFoodAlias(foodId: picked.id, alias: alias);
      } catch (_) {
        // Learning is best-effort; the selection still applies.
      }
    }
    if (!mounted) return;
    setState(() {
      _foodId = picked.id;
      _foodName = picked.name;
    });
  }

  @override
  void dispose() {
    _qty.dispose();
    _unit.dispose();
    _name.dispose();
    _prep.dispose();
    super.dispose();
  }

  void _submit() {
    final name = _name.text.trim();
    if (name.isEmpty) return;
    final base = widget.initial;
    Navigator.pop(
      context,
      Ingredient(
        quantity: double.tryParse(_qty.text.trim().replaceAll(',', '.')),
        unit: _unit.text.trim().isEmpty ? null : _unit.text.trim(),
        name: name,
        prep: _prep.text.trim().isEmpty ? null : _prep.text.trim(),
        ingredientId: base?.ingredientId,
        rawText: base?.rawText,
        foodId: _foodId ?? base?.foodId,
        qualifiers: base?.qualifiers ?? const [],
        resolutionSource: base?.resolutionSource,
        resolutionConfidence: base?.resolutionConfidence,
        needsReview: _foodId != null ? false : (base?.needsReview ?? false),
        raw: false,
        section: base?.section,
        canonicalName: _foodName ?? base?.canonicalName,
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    const gap = SizedBox(height: 12);
    return AlertDialog(
      title: Text(widget.title),
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Row(
              children: [
                Flexible(
                  flex: 2,
                  child: TextField(
                    controller: _qty,
                    decoration: const InputDecoration(
                      labelText: 'Qty',
                      border: OutlineInputBorder(),
                      isDense: true,
                    ),
                    keyboardType: const TextInputType.numberWithOptions(
                      decimal: true,
                    ),
                  ),
                ),
                const SizedBox(width: 8),
                Flexible(
                  flex: 2,
                  child: TextField(
                    controller: _unit,
                    decoration: const InputDecoration(
                      labelText: 'Unit',
                      hintText: 'g, ml…',
                      border: OutlineInputBorder(),
                      isDense: true,
                    ),
                  ),
                ),
              ],
            ),
            gap,
            TextField(
              controller: _name,
              autofocus: true,
              decoration: const InputDecoration(
                labelText: 'Name *',
                border: OutlineInputBorder(),
                isDense: true,
              ),
              onSubmitted: (_) => _submit(),
            ),
            gap,
            TextField(
              controller: _prep,
              decoration: const InputDecoration(
                labelText: 'Prep (optional)',
                hintText: 'diced, sifted…',
                border: OutlineInputBorder(),
                isDense: true,
              ),
              onSubmitted: (_) => _submit(),
            ),
            const SizedBox(height: 8),
            Row(
              children: [
                Expanded(
                  child: OutlinedButton.icon(
                    icon: const Icon(Icons.restaurant, size: 16),
                    label: Text(
                      _foodName != null
                          ? 'Food: $_foodName'
                          : 'Pick food (optional)',
                      overflow: TextOverflow.ellipsis,
                    ),
                    onPressed: _pickFood,
                  ),
                ),
              ],
            ),
          ],
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: const Text('Cancel'),
        ),
        FilledButton(onPressed: _submit, child: const Text('Save')),
      ],
    );
  }
}

// ---------------------------------------------------------------------------
// Instruction edit dialog

class _InstructionDialog extends StatefulWidget {
  final String title;
  final String initialText;

  const _InstructionDialog({required this.title, required this.initialText});

  @override
  State<_InstructionDialog> createState() => _InstructionDialogState();
}

class _InstructionDialogState extends State<_InstructionDialog> {
  late final TextEditingController _ctrl;

  @override
  void initState() {
    super.initState();
    _ctrl = TextEditingController(text: widget.initialText);
  }

  @override
  void dispose() {
    _ctrl.dispose();
    super.dispose();
  }

  void _submit() {
    final steps = splitLines(_ctrl.text);
    if (steps.isEmpty) return;
    Navigator.pop(context, steps);
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: Text(widget.title),
      content: SizedBox(
        width: 420,
        child: TextField(
          controller: _ctrl,
          autofocus: true,
          minLines: 4,
          maxLines: 8,
          decoration: const InputDecoration(
            labelText: 'Instruction',
            hintText: 'Paste multiple lines to create multiple steps',
            border: OutlineInputBorder(),
            alignLabelWithHint: true,
          ),
          onSubmitted: (_) => _submit(),
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: const Text('Cancel'),
        ),
        FilledButton(onPressed: _submit, child: const Text('Save')),
      ],
    );
  }
}

// ---------------------------------------------------------------------------
// Food chooser sheet (identity picker)

class _FoodChooserSheet extends StatefulWidget {
  const _FoodChooserSheet();

  @override
  State<_FoodChooserSheet> createState() => _FoodChooserSheetState();
}

class _FoodChooserSheetState extends State<_FoodChooserSheet> {
  final _ctrl = TextEditingController();
  List<FoodResult> _results = [];
  Timer? _debounce;
  int _seq = 0;

  @override
  void dispose() {
    _ctrl.dispose();
    _debounce?.cancel();
    super.dispose();
  }

  void _onChanged() {
    _debounce?.cancel();
    final q = _ctrl.text.trim();
    if (q.isEmpty) {
      setState(() => _results = []);
      return;
    }
    _debounce = Timer(const Duration(milliseconds: 250), () async {
      final seq = ++_seq;
      try {
        final hits = await fetchFoods(q);
        if (!mounted || seq != _seq) return;
        setState(() => _results = hits);
      } catch (_) {
        if (!mounted || seq != _seq) return;
        setState(() => _results = []);
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    final bottom = MediaQuery.of(context).viewInsets.bottom;
    return Padding(
      padding: EdgeInsets.only(bottom: bottom),
      child: ConstrainedBox(
        constraints: BoxConstraints(
          maxHeight: MediaQuery.of(context).size.height * 0.6,
          minWidth: 320,
        ),
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
                child: TextField(
                  controller: _ctrl,
                  autofocus: true,
                  onChanged: (_) => _onChanged(),
                  decoration: const InputDecoration(
                    hintText: 'Search foods…',
                    border: OutlineInputBorder(),
                    isDense: true,
                  ),
                ),
              ),
              if (_results.isNotEmpty)
                ...List.generate(_results.length, (index) {
                  final food = _results[index];
                  return ListTile(
                    dense: true,
                    visualDensity: VisualDensity.compact,
                    contentPadding: const EdgeInsets.symmetric(horizontal: 16),
                    title: Text(food.name),
                    subtitle: food.category != null
                        ? Text(
                            food.category!,
                            style: const TextStyle(fontSize: 11),
                          )
                        : null,
                    onTap: () => Navigator.pop(context, food),
                  );
                }),
            ],
          ),
        ),
      ),
    );
  }
}
