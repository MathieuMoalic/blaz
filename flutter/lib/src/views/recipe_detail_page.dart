import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:intl/intl.dart';
import 'package:url_launcher/url_launcher.dart';
import 'package:shared_preferences/shared_preferences.dart';
import '../api.dart' as api;
import '../auth.dart';
import 'edit_recipe_page.dart';
import 'login_page.dart';
import 'meal_plan/day_picker_sheet.dart';

class RecipeDetailPage extends StatefulWidget {
  final int recipeId;
  const RecipeDetailPage({super.key, required this.recipeId});

  @override
  State<RecipeDetailPage> createState() => _RecipeDetailPageState();
}

/// Returns true if the ingredient appears unparsed:
/// - explicitly marked raw, OR
/// - has no qty/unit stored but the name itself contains a parseable qty/unit
///   (e.g. "2 tbsp ginger" stored as raw text before the raw field existed)
bool _looksUnparsed(api.Ingredient i) {
  if (i.raw) return true;
  if (i.quantity != null || i.unit != null) return false;
  final parsed = api.parseIngredientLine(i.name);
  return parsed.quantity != null || parsed.unit != null;
}

class _RecipeDetailPageState extends State<RecipeDetailPage> {
  // Precise Atwater factors (kcal per gram).
  static const double kcalPerGProt = 4.27;
  static const double kcalPerGCarb = 3.87;
  static const double kcalPerGFat = 8.79;

  late Future<api.Recipe> _future;

  final Set<int> _checkedIngredients = {};
  final Set<int> _checkedSteps = {};
  double _scale = 1.0;

  bool _estimatingMacros = false;

  // Timer state
  int _timerSeconds = 0;
  bool _timerRunning = false;
  Timer? _timer;

  @override
  void initState() {
    super.initState();
    _future = api.fetchRecipe(widget.recipeId);
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  // ---- Timer ----------------------------------------------------------------

  void _startTimer() {
    if (_timerRunning) return;
    setState(() => _timerRunning = true);
    _timer?.cancel();
    _timer = Timer.periodic(const Duration(seconds: 1), (_) {
      if (_timerSeconds > 0) {
        setState(() => _timerSeconds--);
        if (_timerSeconds == 0) {
          _stopTimer();
          _onTimerComplete();
        }
      }
    });
  }

  void _stopTimer() {
    _timer?.cancel();
    setState(() => _timerRunning = false);
  }

  void _resetTimer() {
    _stopTimer();
    setState(() => _timerSeconds = 0);
  }

  void _setTimer(int seconds) {
    setState(() => _timerSeconds = seconds);
  }

  void _onTimerComplete() {
    // Play a sound or show notification
    HapticFeedback.heavyImpact();
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(
      const SnackBar(
        content: Text('Timer finished!'),
        duration: Duration(seconds: 5),
      ),
    );
  }

  String _formatTime(int totalSeconds) {
    final hours = totalSeconds ~/ 3600;
    final minutes = (totalSeconds % 3600) ~/ 60;
    final seconds = totalSeconds % 60;
    if (hours > 0) {
      return '${hours.toString().padLeft(2, '0')}:${minutes.toString().padLeft(2, '0')}:${seconds.toString().padLeft(2, '0')}';
    }
    return '${minutes.toString().padLeft(2, '0')}:${seconds.toString().padLeft(2, '0')}';
  }

  void _showTimerSheet() {
    showModalBottomSheet(
      context: context,
      builder: (ctx) => _TimerSheet(
        initialSeconds: _timerSeconds,
        running: _timerRunning,
        formatTime: _formatTime,
        onStart: (seconds) {
          _setTimer(seconds);
          _startTimer();
          Navigator.pop(ctx);
        },
        onStop: _stopTimer,
        onReset: () {
          _resetTimer();
          Navigator.pop(ctx);
        },
      ),
    );
  }

  // ---- Helpers --------------------------------------------------------------

  Future<void> _refresh() async {
    final f = api.fetchRecipe(widget.recipeId);
    setState(() => _future = f);
    await f;
  }

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

  double _calcCalories(api.RecipeMacros m) {
    return m.protein * kcalPerGProt +
        m.carbs * kcalPerGCarb +
        m.fat * kcalPerGFat;
  }

  String _fmtTs(String s) {
    try {
      final dt = DateTime.parse(s.replaceFirst(' ', 'T'));
      return DateFormat.yMMMd().add_Hm().format(dt);
    } catch (_) {
      return s;
    }
  }

  Future<bool> _checkAuth(String action) async {
    if (Auth.token != null) return true;
    
    final shouldLogin = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Login Required'),
        content: Text('You need to login to $action.'),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.of(context).pop(true),
            child: const Text('Login'),
          ),
        ],
      ),
    );

    if (shouldLogin == true && mounted) {
      await Navigator.of(context).push(
        MaterialPageRoute(builder: (_) => const LoginPage()),
      );
      return Auth.token != null;
    }
    return false;
  }

  // ---- Actions --------------------------------------------------------------

  Future<void> _reimportFromUrl(api.Recipe r) async {
    if (!await _checkAuth('re-import recipes')) return;
    
    final messenger = ScaffoldMessenger.of(context);
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Re-import from URL?'),
        content: const Text(
          'This will overwrite the title, ingredients, and instructions with freshly imported data. '
          'Notes, images, and the source URL will be kept.',
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('Cancel')),
          FilledButton(onPressed: () => Navigator.pop(ctx, true), child: const Text('Re-import')),
        ],
      ),
    );
    if (confirmed != true) return;
    if (!mounted) return;

    messenger.showSnackBar(
      const SnackBar(content: Text('Re-importing…'), duration: Duration(seconds: 30)),
    );

    try {
      // Get user's selected model
      final prefs = await SharedPreferences.getInstance();
      final model = prefs.getString('llm_model') ?? 'anthropic/claude-3.5-sonnet';
      
      final imported = await api.importRecipeFromUrl(url: r.source, model: model, dryRun: true);
      await api.updateRecipe(
        id: r.id,
        title: imported.title,
        yieldText: imported.yieldText,
        ingredients: imported.ingredients,
        instructions: imported.instructions,
      );
      _refresh();
      messenger
        ..clearSnackBars()
        ..showSnackBar(const SnackBar(content: Text('Re-imported successfully')));
    } catch (e) {
      messenger
        ..clearSnackBars()
        ..showSnackBar(SnackBar(content: Text('Re-import failed: $e')));
    }
  }

  /// Parse raw/unparsed ingredients using LLM, save, then open edit mode for review.
  /// Returns the updated recipe, or null if cancelled/failed.
  Future<api.Recipe?> _parseIngredients(api.Recipe r) async {
    final toParseIndices = [
      for (var i = 0; i < r.ingredients.length; i++)
        if (_looksUnparsed(r.ingredients[i])) i,
    ];
    if (toParseIndices.isEmpty) return r;

    // Step 1: call LLM to get proposed parses for the whole recipe at once.
    List<api.Ingredient> llmResult;
    try {
      if (!mounted) return null;
      // Show a non-dismissible loading dialog while waiting for LLM.
      final future = api.reparseIngredients(r.id);
      showDialog<void>(
        context: context,
        barrierDismissible: false,
        builder: (ctx) => PopScope(
          canPop: false,
          child: AlertDialog(
            content: Row(
              children: [
                const SizedBox(
                  width: 24,
                  height: 24,
                  child: CircularProgressIndicator(strokeWidth: 2),
                ),
                const SizedBox(width: 16),
                Text(
                  'Parsing ${toParseIndices.length} ingredient(s) with AI…',
                ),
              ],
            ),
          ),
        ),
      );
      llmResult = await future;
      if (mounted) Navigator.of(context, rootNavigator: true).pop();
    } catch (e) {
      if (mounted) {
        Navigator.of(context, rootNavigator: true).pop();
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('AI parse failed: $e')),
        );
      }
      return null;
    }

    if (!mounted) return null;

    // Step 2: merge all LLM results and save immediately.
    final ingredients = List<api.Ingredient>.from(r.ingredients);
    for (var pos = 0; pos < toParseIndices.length; pos++) {
      final i = toParseIndices[pos];
      if (i < llmResult.length) {
        ingredients[i] = llmResult[i];
      }
    }

    api.Recipe updated;
    try {
      updated = await api.updateRecipe(id: r.id, ingredients: ingredients);
      _future = Future.value(updated);
      if (mounted) setState(() {});
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Failed to save: $e')),
        );
      }
      return null;
    }

    if (!mounted) return null;

    // Step 3: open edit page so the user can review and adjust parsed ingredients.
    await Navigator.of(context).push(
      MaterialPageRoute(
        builder: (_) => EditRecipePage(recipe: updated),
      ),
    );
    if (mounted) {
      _future = api.fetchRecipe(widget.recipeId);
      setState(() {});
    }

    return updated;
  }

  Future<void> _addIngredients(api.Recipe r) async {
    if (!await _checkAuth('add ingredients to shopping list')) return;
    
    if (r.ingredients.isEmpty) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('No ingredients to add')),
      );
      return;
    }

    var recipe = r;
    if (recipe.ingredients.any(_looksUnparsed)) {
      if (!mounted) return;
      final start = await showDialog<bool>(
        context: context,
        builder: (ctx) => AlertDialog(
          title: const Text('Unparsed ingredients'),
          content: const Text(
            'Some ingredients haven\'t been parsed yet. '
            'Parse them now before adding to the shopping list?',
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () => Navigator.pop(ctx, true),
              child: const Text('Start Parsing'),
            ),
          ],
        ),
      );
      if (start != true || !mounted) return;
      final updated = await _parseIngredients(recipe);
      if (updated == null || !mounted) return;
      recipe = updated;
      if (recipe.ingredients.any(_looksUnparsed)) return; // still unparsed
    }

    if (!mounted) return;
    final selected = await _pickIngredientsSheet(
      recipe.ingredients.where((ing) => !ing.isSection).toList(),
    );

    if (!mounted) return;
    if (selected == null || selected.isEmpty) return;

    try {
      await api.mergeShoppingIngredients(selected, recipeId: r.id);
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Added ${selected.length} item(s)')),
      );
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Failed to add: $e')),
      );
    }
  }

  Future<void> _assignToMealPlan(api.Recipe r) async {
    if (!await _checkAuth('add recipes to meal plan')) return;
    
    final day = await showDayPickerSheet(
      context: context,
      recipeTitle: r.title,
    );

    if (!mounted) return;
    if (day == null) return;

    try {
      final entry = await api.assignRecipeToDay(
        day: day,
        recipeId: r.id,
      );
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Assigned “${r.title}” to ${entry.day}')),
      );
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(SnackBar(content: Text('Failed to assign: $e')));
    }
  }

  Future<void> _estimateMacros() async {
    if (!await _checkAuth('estimate macros')) return;
    
    if (_estimatingMacros) return;
    // Never mark this callback async inside setState.
    setState(() {
      _estimatingMacros = true;
    });

    api.Recipe? updated;
    try {
      updated = await api.estimateRecipeMacros(widget.recipeId);
      if (!mounted) return;
      // Assign future outside the setState body, then trigger rebuild.
      _future = Future.value(updated);
      setState(() {});
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(const SnackBar(content: Text('Macros estimated')));
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(SnackBar(content: Text('Macro estimation failed: $e')));
    } finally {
      if (mounted) {
        setState(() {
          _estimatingMacros = false;
        });
      }
    }
  }

  Future<void> _shareRecipe(api.Recipe r) async {
    if (!await _checkAuth('share recipes')) return;
    
    try {
      final token = await api.shareRecipe(r.id);
      // Build share URL from the current base URL
      final base = api.baseUrl.replaceAll(RegExp(r'/$'), '');
      final link = '$base/share/$token';
      if (!mounted) return;
      await showDialog<void>(
        context: context,
        builder: (ctx) => AlertDialog(
          title: const Text('Share link'),
          content: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              SelectableText(link, style: const TextStyle(fontSize: 13)),
              const SizedBox(height: 12),
              if (r.shareToken != null)
                TextButton.icon(
                  icon: const Icon(Icons.link_off),
                  label: const Text('Revoke link'),
                  onPressed: () async {
                    await api.revokeRecipeShare(r.id);
                    if (ctx.mounted) Navigator.pop(ctx);
                    _refresh();
                  },
                ),
            ],
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(ctx),
              child: const Text('Close'),
            ),
            FilledButton.icon(
              icon: const Icon(Icons.copy),
              label: const Text('Copy'),
              onPressed: () {
                Clipboard.setData(ClipboardData(text: link));
                Navigator.pop(ctx);
                ScaffoldMessenger.of(context).showSnackBar(
                  const SnackBar(content: Text('Link copied')),
                );
              },
            ),
          ],
        ),
      );
      _refresh();
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Failed to create share link: $e')),
      );
    }
  }

  Future<void> _confirmDelete(api.Recipe r) async {
    if (!await _checkAuth('delete recipes')) return;
    
    // Check for upcoming meal plan entries before showing the dialog.
    List<api.MealPlanEntry> upcoming = [];
    try {
      upcoming = await api.fetchMealPlanForRecipe(r.id);
    } catch (_) {
      // If the check fails, proceed with the generic confirmation.
    }

    if (!mounted) return;

    final String dialogContent;
    if (upcoming.isEmpty) {
      dialogContent = 'Are you sure you want to delete "${r.title}"?';
    } else {
      final dates = upcoming.map((e) => e.day).join(', ');
      dialogContent = '"${r.title}" is scheduled on your meal plan ($dates). '
          'Deleting it will also remove those entries.';
    }

    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Delete recipe?'),
        content: Text(dialogContent),
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
        await api.deleteRecipe(r.id);
        if (!mounted) return;
        Navigator.of(context).pop(true);
        ScaffoldMessenger.of(
          context,
        ).showSnackBar(SnackBar(content: Text('Deleted "${r.title}"')));
      } catch (e) {
        if (!mounted) return;
        ScaffoldMessenger.of(
          context,
        ).showSnackBar(SnackBar(content: Text('Failed to delete: $e')));
      }
    }
  }

  /// Bottom sheet to select and optionally rename ingredients before adding to the shopping list.
  Future<List<api.Ingredient>?> _pickIngredientsSheet(
    List<api.Ingredient> items,
  ) async {
    return showModalBottomSheet<List<api.Ingredient>>(
      context: context,
      isScrollControlled: true,
      showDragHandle: true,
      builder: (ctx) => _AddIngredientsSheet(items: items, scale: _scale),
    );
  }

  void _openImageViewer({required String fullUrl, required String heroTag}) {
    Navigator.of(context, rootNavigator: true).push(
      PageRouteBuilder(
        opaque: false,
        barrierColor: Colors.black.withValues(alpha: 0.95),
        pageBuilder: (_, __, ___) =>
            _ImageViewerPage(url: fullUrl, heroTag: heroTag),
        transitionsBuilder: (_, anim, __, child) =>
            FadeTransition(opacity: anim, child: child),
      ),
    );
  }

  // ---- UI -------------------------------------------------------------------

  @override
  Widget build(BuildContext context) {
    final isAuthenticated = Auth.token != null;
    
    return Scaffold(
      appBar: AppBar(
        actions: [
          // Timer button (always visible, discrete)
          Stack(
            alignment: Alignment.center,
            children: [
              IconButton(
                tooltip: 'Timer',
                icon: Icon(
                  _timerRunning ? Icons.timer : Icons.timer_outlined,
                  color: _timerRunning ? Theme.of(context).colorScheme.primary : null,
                ),
                onPressed: _showTimerSheet,
              ),
              if (_timerSeconds > 0)
                Positioned(
                  right: 4,
                  top: 4,
                  child: Container(
                    padding: const EdgeInsets.all(2),
                    decoration: BoxDecoration(
                      color: Theme.of(context).colorScheme.primary,
                      borderRadius: BorderRadius.circular(6),
                    ),
                    constraints: const BoxConstraints(minWidth: 12, minHeight: 12),
                  ),
                ),
            ],
          ),
          if (isAuthenticated) ...[
            FutureBuilder<api.Recipe>(
              future: _future,
              builder: (context, snap) {
                final isUrl = snap.hasData &&
                    snap.data!.source.startsWith('http');
                if (!isUrl) return const SizedBox.shrink();
                return IconButton(
                  tooltip: 'Re-import from URL',
                  icon: const Icon(Icons.refresh),
                  onPressed: () => _reimportFromUrl(snap.data!),
                );
              },
            ),
            IconButton(
              tooltip: 'Share',
              icon: const Icon(Icons.share_outlined),
              onPressed: () async {
                final r = await _future;
                if (!mounted) return;
                _shareRecipe(r);
              },
            ),
            IconButton(
              tooltip: 'Add to meal plan',
              icon: const Icon(Icons.event_outlined),
              onPressed: () async {
                final r = await _future;
                if (!mounted) return;
                _assignToMealPlan(r);
              },
            ),
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
                if (!context.mounted) return;

                final changed = await Navigator.push<bool>(
                  context,
                  MaterialPageRoute(builder: (_) => EditRecipePage(recipe: r)),
                );
                if (!mounted) return;

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
        ],
      ),
      body: Stack(
        children: [
          FutureBuilder<api.Recipe>(
            future: _future,
            builder: (context, snap) {
              if (snap.connectionState != ConnectionState.done) {
                return const Center(child: CircularProgressIndicator());
              }
              if (snap.hasError) {
                return Center(child: Text('Error: ${snap.error}'));
              }
              final r = snap.data!;
              final small = api.mediaUrl(r.imagePathSmall);
              final full = api.mediaUrl(r.imagePathFull);
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
                Card(
                  margin: const EdgeInsets.only(top: 4, bottom: 12),
                  child: Padding(
                    padding: const EdgeInsets.all(16),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          'Ingredients',
                          style: Theme.of(context).textTheme.titleMedium,
                        ),
                        const SizedBox(height: 8),
                        if (r.ingredients.any(_looksUnparsed)) ...[
                          Container(
                            margin: const EdgeInsets.only(bottom: 10),
                            padding: const EdgeInsets.symmetric(
                              horizontal: 12,
                              vertical: 10,
                            ),
                            decoration: BoxDecoration(
                              color: Theme.of(context).colorScheme.secondaryContainer,
                              borderRadius: BorderRadius.circular(8),
                            ),
                            child: Row(
                              children: [
                                Icon(
                                  Icons.warning_amber_rounded,
                                  size: 18,
                                  color: Theme.of(context).colorScheme.onSecondaryContainer,
                                ),
                                const SizedBox(width: 8),
                                Expanded(
                                  child: Text(
                                    'Some ingredients are not yet parsed',
                                    style: TextStyle(
                                      fontSize: 13,
                                      color: Theme.of(context).colorScheme.onSecondaryContainer,
                                    ),
                                  ),
                                ),
                                const SizedBox(width: 8),
                                FilledButton.tonal(
                                  onPressed: () async {
                                    final updated = await _parseIngredients(r);
                                    if (updated == null) return;
                                  },
                                  style: FilledButton.styleFrom(
                                    visualDensity: VisualDensity.compact,
                                    padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
                                  ),
                                  child: const Text('Parse', style: TextStyle(fontSize: 13)),
                                ),
                              ],
                            ),
                          ),
                        ],
                        Row(
                          children: [
                            Text(
                              'Scale',
                              style: Theme.of(context).textTheme.bodyLarge,
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
                        const SizedBox(height: 8),
                        if (r.ingredients.isEmpty)
                          const Text('—')
                        else
                          ...r.ingredients.asMap().entries.map((e) {
                            final idx = e.key;
                            final ing = e.value;
                            if (ing.isSection) {
                              return Padding(
                                padding: const EdgeInsets.only(top: 10, bottom: 2),
                                child: Text(
                                  ing.section!,
                                  style: Theme.of(context).textTheme.labelLarge?.copyWith(
                                    color: Theme.of(context).colorScheme.primary,
                                    letterSpacing: 0.5,
                                  ),
                                ),
                              );
                            }
                            final line = ing.toLine(factor: _scale);
                            final checked = _checkedIngredients.contains(idx);
                            return _Bullet(
                              text: line,
                              checked: checked,
                              onTap: () => _toggleIngredient(idx),
                            );
                          }),
                      ],
                    ),
                  ),
                ),

                // Instructions
                Card(
                  margin: const EdgeInsets.only(bottom: 12),
                  child: Padding(
                    padding: const EdgeInsets.all(16),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          'Instructions',
                          style: Theme.of(context).textTheme.titleMedium,
                        ),
                        const SizedBox(height: 8),
                        if (r.instructions.isEmpty)
                          const Text('—')
                        else
                          ...() {
                            final widgets = <Widget>[];
                            var stepNum = 0;
                            for (var i = 0; i < r.instructions.length; i++) {
                              final text = r.instructions[i];
                              if (text.startsWith('## ')) {
                                widgets.add(Padding(
                                  padding: const EdgeInsets.only(top: 10, bottom: 2),
                                  child: Text(
                                    text.substring(3),
                                    style: Theme.of(context).textTheme.labelLarge?.copyWith(
                                      color: Theme.of(context).colorScheme.primary,
                                      letterSpacing: 0.5,
                                    ),
                                  ),
                                ));
                              } else {
                                stepNum++;
                                widgets.add(_Numbered(
                                  step: stepNum,
                                  text: text,
                                  checked: _checkedSteps.contains(i),
                                  onTap: () => _toggleStep(i),
                                ));
                              }
                            }
                            return widgets;
                          }(),
                      ],
                    ),
                  ),
                ),

                // Prep Reminders
                _PrepRemindersSection(
                  recipe: r,
                  onChanged: _refresh,
                ),

                // Notes
                if (r.notes.isNotEmpty)
                  Card(
                    margin: const EdgeInsets.only(bottom: 12),
                    child: Padding(
                      padding: const EdgeInsets.all(16),
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text('Notes', style: Theme.of(context).textTheme.titleMedium),
                          const SizedBox(height: 8),
                          Text(r.notes, style: Theme.of(context).textTheme.bodyLarge),
                        ],
                      ),
                    ),
                  ),

                // Meta
                const SizedBox(height: 16),
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
                  value: r.createdAt.isEmpty ? '—' : _fmtTs(r.createdAt),
                ),
                _MetaRow(
                  label: 'Updated',
                  value: r.updatedAt.isEmpty ? '—' : _fmtTs(r.updatedAt),
                ),

                // Macros & Calories
                const SizedBox(height: 18),
                _MacrosSection(
                  macros: r.macros,
                  estimating: _estimatingMacros,
                  onEstimate: _estimateMacros,
                  calcCalories: _calcCalories,
                ),
              ],
            ),
          );
        },
      ),
      // Persistent timer display when timer is running or set
      if (_timerSeconds > 0 || _timerRunning)
        Positioned(
          bottom: 16,
          left: 16,
          right: 16,
          child: GestureDetector(
            onTap: _showTimerSheet,
            child: Card(
              elevation: 8,
              child: Padding(
                padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
                child: Row(
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: [
                    Icon(
                      _timerRunning ? Icons.timer : Icons.timer_outlined,
                      color: _timerRunning
                          ? Theme.of(context).colorScheme.primary
                          : null,
                    ),
                    const SizedBox(width: 12),
                    Text(
                      _formatTime(_timerSeconds),
                      style: Theme.of(context).textTheme.headlineMedium?.copyWith(
                        fontWeight: FontWeight.w500,
                        fontFeatures: const [FontFeature.tabularFigures()],
                        color: _timerRunning
                            ? Theme.of(context).colorScheme.primary
                            : null,
                      ),
                    ),
                    const SizedBox(width: 12),
                    if (_timerRunning)
                      IconButton(
                        icon: const Icon(Icons.pause),
                        onPressed: _stopTimer,
                        tooltip: 'Pause',
                      )
                    else
                      IconButton(
                        icon: const Icon(Icons.play_arrow),
                        onPressed: _timerSeconds > 0 ? _startTimer : null,
                        tooltip: 'Start',
                      ),
                    IconButton(
                      icon: const Icon(Icons.close),
                      onPressed: _resetTimer,
                      tooltip: 'Reset',
                    ),
                  ],
                ),
              ),
            ),
          ),
        ),
      ],
    ),
    );
  }
}

// ---- Small UI helpers -------------------------------------------------------

class _PrepRemindersSection extends StatefulWidget {
  final api.Recipe recipe;
  final VoidCallback onChanged;
  const _PrepRemindersSection({required this.recipe, required this.onChanged});

  @override
  State<_PrepRemindersSection> createState() => _PrepRemindersSectionState();
}

class _PrepRemindersSectionState extends State<_PrepRemindersSection> {
  bool _saving = false;
  late List<api.PrepReminder> _reminders;

  @override
  void initState() {
    super.initState();
    _reminders = List<api.PrepReminder>.from(widget.recipe.prepReminders);
  }

  @override
  void didUpdateWidget(_PrepRemindersSection oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.recipe != widget.recipe) {
      _reminders = List<api.PrepReminder>.from(widget.recipe.prepReminders);
    }
  }

  Future<void> _save(List<api.PrepReminder> reminders) async {
    setState(() {
      _saving = true;
      _reminders = reminders;
    });
    try {
      await api.updateRecipePrepReminders(
        id: widget.recipe.id,
        prepReminders: reminders,
      );
      widget.onChanged();
    } catch (e) {
      if (mounted) {
        setState(() => _reminders =
            List<api.PrepReminder>.from(widget.recipe.prepReminders));
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Failed to save: $e')));
      }
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  Future<void> _showEditDialog({api.PrepReminder? existing, int? index}) async {
    final stepCtrl = TextEditingController(text: existing?.step ?? '');
    int hoursBefore = existing?.hoursBefore ?? 12;
    bool deleted = false;

    final result = await showDialog<api.PrepReminder?>(
      context: context,
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, setDialog) => AlertDialog(
          title: Text(existing == null ? 'Add prep reminder' : 'Edit reminder'),
          content: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              TextField(
                controller: stepCtrl,
                autofocus: true,
                decoration: const InputDecoration(
                  labelText: 'What to do',
                  hintText: 'e.g. Soak beans overnight',
                  border: OutlineInputBorder(),
                ),
                textCapitalization: TextCapitalization.sentences,
              ),
              const SizedBox(height: 16),
              Text('How far in advance',
                  style: Theme.of(ctx).textTheme.labelMedium),
              const SizedBox(height: 8),
              Wrap(
                spacing: 8,
                children: [2, 4, 8, 12, 24, 48].map((h) {
                  final label = h < 24 ? '${h}h' : '${h ~/ 24}d';
                  return ChoiceChip(
                    label: Text(label),
                    selected: hoursBefore == h,
                    onSelected: (_) => setDialog(() => hoursBefore = h),
                  );
                }).toList(),
              ),
            ],
          ),
          actions: [
            if (existing != null)
              TextButton(
                onPressed: () {
                  deleted = true;
                  Navigator.pop(ctx);
                },
                style: TextButton.styleFrom(
                    foregroundColor: Theme.of(ctx).colorScheme.error),
                child: const Text('Delete'),
              ),
            TextButton(
              onPressed: () => Navigator.pop(ctx),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () {
                final step = stepCtrl.text.trim();
                if (step.isEmpty) return;
                Navigator.pop(
                    ctx, api.PrepReminder(step: step, hoursBefore: hoursBefore));
              },
              child: const Text('Save'),
            ),
          ],
        ),
      ),
    );

    if (!mounted) return;

    final current = List<api.PrepReminder>.from(_reminders);

    if (deleted && existing != null && index != null) {
      current.removeAt(index);
      await _save(current);
    } else if (result != null) {
      if (index != null) {
        current[index] = result;
      } else {
        current.add(result);
      }
      await _save(current);
    }
  }

  String _hoursLabel(int h) =>
      h < 24 ? '${h}h before' : '${h ~/ 24}d before';

  @override
  Widget build(BuildContext context) {
    final reminders = _reminders;
    final theme = Theme.of(context);

    return Card(
      margin: const EdgeInsets.only(bottom: 12),
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Text('Prep Reminders', style: theme.textTheme.titleMedium),
                const Spacer(),
                if (_saving)
                  const SizedBox(
                      width: 18,
                      height: 18,
                      child: CircularProgressIndicator(strokeWidth: 2))
                else
                  IconButton(
                    icon: const Icon(Icons.add),
                    tooltip: 'Add reminder',
                    visualDensity: VisualDensity.compact,
                    onPressed: () => _showEditDialog(),
                  ),
              ],
            ),
            if (reminders.isEmpty)
              Padding(
                padding: const EdgeInsets.only(top: 4),
                child: Text(
                  'No advance prep needed',
                  style: theme.textTheme.bodySmall?.copyWith(
                    color: theme.colorScheme.onSurfaceVariant,
                  ),
                ),
              )
            else
              for (int i = 0; i < reminders.length; i++)
                ListTile(
                  dense: true,
                  contentPadding: EdgeInsets.zero,
                  leading: const Icon(Icons.alarm_outlined, size: 20),
                  title: Text(reminders[i].step),
                  trailing: Text(
                    _hoursLabel(reminders[i].hoursBefore),
                    style: theme.textTheme.bodySmall,
                  ),
                  onTap: () => _showEditDialog(existing: reminders[i], index: i),
                ),
          ],
        ),
      ),
    );
  }
}

class _MetaRow extends StatelessWidget {
  final String label;
  final String value;
  const _MetaRow({required this.label, required this.value});
  
  bool get _isUrl => value.startsWith('http://') || value.startsWith('https://');
  
  Future<void> _launchUrl() async {
    final uri = Uri.parse(value);
    if (!await launchUrl(uri, mode: LaunchMode.externalApplication)) {
      throw Exception('Could not launch $value');
    }
  }
  
  @override
  Widget build(BuildContext context) {
    final styleLabel = Theme.of(context).textTheme.bodySmall;
    final styleValue = Theme.of(context).textTheme.bodyMedium;
    final colorScheme = Theme.of(context).colorScheme;
    
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 2),
      child: Row(
        children: [
          SizedBox(width: 90, child: Text(label, style: styleLabel)),
          Expanded(
            child: _isUrl
                ? InkWell(
                    onTap: _launchUrl,
                    child: Text(
                      value,
                      style: styleValue?.copyWith(
                        color: colorScheme.primary,
                        decoration: TextDecoration.underline,
                      ),
                    ),
                  )
                : Text(value, style: styleValue),
          ),
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
    final base = Theme.of(context).textTheme.bodyLarge;  // Changed from bodyMedium to bodyLarge
    final style = base?.copyWith(
      decoration: checked ? TextDecoration.lineThrough : null,
      color: checked
          ? (base.color ?? Colors.black).withValues(alpha: 0.55)
          : base.color,
      height: 1.3,  // Tighter line height
    );
    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(6),
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 4),  // Reduced from 6
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('•  ', style: base),
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
    final base = Theme.of(context).textTheme.bodyLarge;  // Changed from bodyMedium to bodyLarge
    final style = base?.copyWith(
      decoration: checked ? TextDecoration.lineThrough : null,
      color: checked
          ? (base.color ?? Colors.black).withValues(alpha: 0.55)
          : base.color,
      height: 1.3,  // Tighter line height
    );
    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(6),
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 6),  // Slightly more padding for steps
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('$step. ', style: base),
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

class _MacrosSection extends StatelessWidget {
  final api.RecipeMacros? macros;
  final bool estimating;
  final VoidCallback onEstimate;
  final double Function(api.RecipeMacros) calcCalories;

  const _MacrosSection({
    required this.macros,
    required this.estimating,
    required this.onEstimate,
    required this.calcCalories,
  });

  @override
  Widget build(BuildContext context) {
    final t = Theme.of(context).textTheme;

    if (macros == null) {
      return Row(
        children: [
          FilledButton.icon(
            onPressed: estimating ? null : onEstimate,
            icon: estimating
                ? const SizedBox(
                    width: 16,
                    height: 16,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                : const Icon(Icons.calculate_outlined),
            label: const Text('Estimate macros'),
          ),
        ],
      );
    }

    final m = macros!;
    final kcal = calcCalories(m).clamp(0, double.infinity);

    double calcIngredientCalories(api.IngredientMacros ing) {
      return (ing.protein * 4.27 + ing.fat * 8.79 + ing.carbs * 3.87).clamp(0, double.infinity);
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text('Nutrition (per recipe)', style: t.titleMedium),
        const SizedBox(height: 8),
        Card(
          child: Padding(
            padding: const EdgeInsets.all(12),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Table(
                  columnWidths: const {
                    0: FlexColumnWidth(3),
                    1: FixedColumnWidth(55),
                    2: FixedColumnWidth(55),
                    3: FixedColumnWidth(55),
                    4: FixedColumnWidth(60),
                  },
                  defaultVerticalAlignment: TableCellVerticalAlignment.middle,
                  children: [
                    TableRow(
                      children: [
                        Padding(
                          padding: const EdgeInsets.only(bottom: 4),
                          child: Text('Ingredient', style: t.labelSmall),
                        ),
                        Padding(
                          padding: const EdgeInsets.only(bottom: 4),
                          child: Text('P (g)', style: t.labelSmall, textAlign: TextAlign.right),
                        ),
                        Padding(
                          padding: const EdgeInsets.only(bottom: 4),
                          child: Text('F (g)', style: t.labelSmall, textAlign: TextAlign.right),
                        ),
                        Padding(
                          padding: const EdgeInsets.only(bottom: 4),
                          child: Text('C (g)', style: t.labelSmall, textAlign: TextAlign.right),
                        ),
                        Padding(
                          padding: const EdgeInsets.only(bottom: 4),
                          child: Text('kcal', style: t.labelSmall, textAlign: TextAlign.right),
                        ),
                      ],
                    ),
                    TableRow(
                      decoration: BoxDecoration(
                        border: Border(
                          bottom: BorderSide(
                            color: Theme.of(context).dividerColor,
                            width: 1,
                          ),
                        ),
                      ),
                      children: [
                        Padding(
                          padding: const EdgeInsets.symmetric(vertical: 6),
                          child: Text('Total', style: t.titleSmall),
                        ),
                        Padding(
                          padding: const EdgeInsets.symmetric(vertical: 6),
                          child: Text(
                            m.protein.round().toString(),
                            style: t.bodyMedium?.copyWith(
                              fontWeight: FontWeight.bold,
                              fontFeatures: [const FontFeature.tabularFigures()],
                            ),
                            textAlign: TextAlign.right,
                          ),
                        ),
                        Padding(
                          padding: const EdgeInsets.symmetric(vertical: 6),
                          child: Text(
                            m.fat.round().toString(),
                            style: t.bodyMedium?.copyWith(
                              fontWeight: FontWeight.bold,
                              fontFeatures: [const FontFeature.tabularFigures()],
                            ),
                            textAlign: TextAlign.right,
                          ),
                        ),
                        Padding(
                          padding: const EdgeInsets.symmetric(vertical: 6),
                          child: Text(
                            m.carbs.round().toString(),
                            style: t.bodyMedium?.copyWith(
                              fontWeight: FontWeight.bold,
                              fontFeatures: [const FontFeature.tabularFigures()],
                            ),
                            textAlign: TextAlign.right,
                          ),
                        ),
                        Padding(
                          padding: const EdgeInsets.symmetric(vertical: 6),
                          child: Text(
                            kcal.toStringAsFixed(0),
                            style: t.bodyMedium?.copyWith(
                              fontWeight: FontWeight.bold,
                              fontFeatures: [const FontFeature.tabularFigures()],
                            ),
                            textAlign: TextAlign.right,
                          ),
                        ),
                      ],
                    ),
                    ...m.ingredients.where((ing) => !ing.skipped).map((ing) {
                      final ingKcal = calcIngredientCalories(ing);
                      return TableRow(
                        children: [
                          Padding(
                            padding: const EdgeInsets.symmetric(vertical: 4),
                            child: Text(ing.name, style: t.bodyMedium),
                          ),
                          Padding(
                            padding: const EdgeInsets.symmetric(vertical: 4),
                            child: Text(
                              ing.protein.round().toString(),
                              style: t.bodySmall?.copyWith(
                                fontFeatures: [const FontFeature.tabularFigures()],
                              ),
                              textAlign: TextAlign.right,
                            ),
                          ),
                          Padding(
                            padding: const EdgeInsets.symmetric(vertical: 4),
                            child: Text(
                              ing.fat.round().toString(),
                              style: t.bodySmall?.copyWith(
                                fontFeatures: [const FontFeature.tabularFigures()],
                              ),
                              textAlign: TextAlign.right,
                            ),
                          ),
                          Padding(
                            padding: const EdgeInsets.symmetric(vertical: 4),
                            child: Text(
                              ing.carbs.round().toString(),
                              style: t.bodySmall?.copyWith(
                                fontFeatures: [const FontFeature.tabularFigures()],
                              ),
                              textAlign: TextAlign.right,
                            ),
                          ),
                          Padding(
                            padding: const EdgeInsets.symmetric(vertical: 4),
                            child: Text(
                              ingKcal.toStringAsFixed(0),
                              style: t.bodySmall?.copyWith(
                                fontFeatures: [const FontFeature.tabularFigures()],
                              ),
                              textAlign: TextAlign.right,
                            ),
                          ),
                        ],
                      );
                    }),
                  ],
                ),
              ],
            ),
          ),
        ),
        const SizedBox(height: 8),
        Row(
          children: [
            OutlinedButton.icon(
              onPressed: estimating ? null : onEstimate,
              icon: estimating
                  ? const SizedBox(
                      width: 16,
                      height: 16,
                      child: CircularProgressIndicator(strokeWidth: 2),
                    )
                  : const Icon(Icons.refresh),
              label: const Text('Re-estimate'),
            ),
          ],
        ),
      ],
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
        color: Colors.black.withValues(alpha: 0.95),
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

// ---------------------------------------------------------------------------
// Add-to-shopping-list bottom sheet

class _AddIngredientsSheet extends StatefulWidget {
  final List<api.Ingredient> items;
  final double scale;

  const _AddIngredientsSheet({required this.items, required this.scale});

  @override
  State<_AddIngredientsSheet> createState() => _AddIngredientsSheetState();
}

class _AddIngredientsSheetState extends State<_AddIngredientsSheet> {
  late List<bool> _selected;

  @override
  void initState() {
    super.initState();
    _selected = List.filled(widget.items.length, true);
  }

  bool get _anySelected => _selected.any((v) => v);
  bool get _allSelected => _selected.every((v) => v);

  static String _fmtQty(double v) {
    final s = ((v * 100).round() / 100.0).toString();
    return s.endsWith('.0') ? s.replaceFirst('.0', '') : s;
  }

  String _tileLabel(int i) {
    final ing = widget.items[i];
    final q = ing.quantity != null ? ing.quantity! * widget.scale : null;
    final qStr = q != null ? _fmtQty(q) : '';
    final uStr = ing.unit ?? '';
    final sep = (qStr.isNotEmpty && uStr.isNotEmpty) ? '\u00a0' : '';
    final prefix = '$qStr$sep$uStr';
    return prefix.isNotEmpty ? '$prefix  ${ing.name}' : ing.name;
  }

  void _confirm() {
    final result = <api.Ingredient>[];
    for (var i = 0; i < widget.items.length; i++) {
      if (!_selected[i]) continue;
      final ing = widget.items[i];
      result.add(api.Ingredient(
        quantity: ing.quantity != null ? ing.quantity! * widget.scale : null,
        unit: ing.unit,
        name: ing.name,
      ));
    }
    Navigator.pop(context, result);
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final media = MediaQuery.of(context);

    return SafeArea(
      child: SizedBox(
        height: media.size.height * 0.7,
        child: Column(
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 10, 16, 6),
              child: Row(
                children: [
                  Expanded(
                    child: Text('Add to shopping list', style: theme.textTheme.titleMedium),
                  ),
                  Checkbox(
                    value: _allSelected ? true : (_anySelected ? null : false),
                    tristate: true,
                    onChanged: (_) {
                      final target = !_allSelected;
                      setState(() {
                        for (var i = 0; i < _selected.length; i++) {
                          _selected[i] = target;
                        }
                      });
                    },
                  ),
                  Text('All', style: theme.textTheme.bodySmall),
                ],
              ),
            ),
            const Divider(height: 1),
            Expanded(
              child: ListView.builder(
                padding: const EdgeInsets.symmetric(vertical: 4),
                itemCount: widget.items.length,
                itemBuilder: (_, i) => CheckboxListTile(
                  value: _selected[i],
                  onChanged: (v) => setState(() => _selected[i] = v ?? false),
                  controlAffinity: ListTileControlAffinity.leading,
                  dense: true,
                  title: Text(_tileLabel(i)),
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
                      onPressed: () => Navigator.pop(context),
                      child: const Text('Cancel'),
                    ),
                  ),
                  const SizedBox(width: 12),
                  Expanded(
                    child: FilledButton.icon(
                      onPressed: _anySelected ? _confirm : null,
                      icon: const Icon(Icons.shopping_cart_outlined),
                      label: Text(
                        _anySelected
                            ? 'Add ${_selected.where((s) => s).length}'
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
  }
}

// ---------------------------------------------------------------------------
// Simple rename dialog — owns its own TextEditingController

class _RenameDialog extends StatefulWidget {
  final String initial;
  const _RenameDialog({required this.initial});

  @override
  State<_RenameDialog> createState() => _RenameDialogState();
}

class _RenameDialogState extends State<_RenameDialog> {
  late final TextEditingController _ctrl;

  @override
  void initState() {
    super.initState();
    _ctrl = TextEditingController(text: widget.initial);
  }

  @override
  void dispose() {
    _ctrl.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Text('Shopping name'),
      content: TextField(
        controller: _ctrl,
        autofocus: true,
        decoration: const InputDecoration(border: OutlineInputBorder(), isDense: true),
        onSubmitted: (_) => Navigator.pop(context, _ctrl.text.trim()),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context), child: const Text('Cancel')),
        FilledButton(
          onPressed: () => Navigator.pop(context, _ctrl.text.trim()),
          child: const Text('OK'),
        ),
      ],
    );
  }
}

// ---------------------------------------------------------------------------
// Timer bottom sheet

class _TimerSheet extends StatefulWidget {
  final int initialSeconds;
  final bool running;
  final String Function(int) formatTime;
  final void Function(int seconds) onStart;
  final VoidCallback onStop;
  final VoidCallback onReset;

  const _TimerSheet({
    required this.initialSeconds,
    required this.running,
    required this.formatTime,
    required this.onStart,
    required this.onStop,
    required this.onReset,
  });

  @override
  State<_TimerSheet> createState() => _TimerSheetState();
}

class _TimerSheetState extends State<_TimerSheet> {
  late int _seconds;

  @override
  void initState() {
    super.initState();
    _seconds = widget.initialSeconds;
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);

    return Padding(
      padding: const EdgeInsets.all(24),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          // Timer display
          Text(
            widget.formatTime(_seconds),
            style: theme.textTheme.displayLarge?.copyWith(
              fontWeight: FontWeight.w300,
              fontFeatures: const [FontFeature.tabularFigures()],
              color: widget.running ? theme.colorScheme.primary : null,
            ),
          ),
          const SizedBox(height: 24),

          // Quick presets
          Wrap(
            spacing: 8,
            runSpacing: 8,
            alignment: WrapAlignment.center,
            children: [
              for (final (label, secs) in [
                ('1m', 60),
                ('3m', 180),
                ('5m', 300),
                ('10m', 600),
                ('15m', 900),
                ('30m', 1800),
              ])
                ActionChip(
                  label: Text(label),
                  onPressed: () => setState(() => _seconds = secs),
                ),
            ],
          ),
          const SizedBox(height: 24),

          // Controls
          Row(
            mainAxisAlignment: MainAxisAlignment.spaceEvenly,
            children: [
              // Reset
              IconButton.filled(
                onPressed: _seconds > 0 ? widget.onReset : null,
                icon: const Icon(Icons.refresh),
                tooltip: 'Reset',
                style: IconButton.styleFrom(
                  backgroundColor: theme.colorScheme.surfaceContainerHighest,
                  foregroundColor: theme.colorScheme.onSurface,
                ),
              ),

              // Play/Pause (larger)
              IconButton.filled(
                onPressed: _seconds > 0
                    ? (widget.running ? widget.onStop : () => widget.onStart(_seconds))
                    : null,
                icon: Icon(widget.running ? Icons.pause : Icons.play_arrow, size: 32),
                tooltip: widget.running ? 'Pause' : 'Start',
                style: IconButton.styleFrom(
                  minimumSize: const Size(64, 64),
                  backgroundColor: theme.colorScheme.primary,
                  foregroundColor: theme.colorScheme.onPrimary,
                ),
              ),

              // +1 minute
              IconButton.filled(
                onPressed: () => setState(() => _seconds += 60),
                icon: const Icon(Icons.add),
                tooltip: 'Add 1 minute',
                style: IconButton.styleFrom(
                  backgroundColor: theme.colorScheme.surfaceContainerHighest,
                  foregroundColor: theme.colorScheme.onSurface,
                ),
              ),
            ],
          ),
          const SizedBox(height: 16),
        ],
      ),
    );
  }
}
