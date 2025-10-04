package com.example.hellocompose

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.example.hellocompose.api.ApiClient
import com.example.hellocompose.api.Recipe

sealed interface UiState {
  object Loading : UiState
  data class Data(val items: List<Recipe>) : UiState
  data class Error(val message: String) : UiState
}

class MainActivity : ComponentActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    setContent {
      MaterialTheme { Surface(Modifier.fillMaxSize()) { RecipesScreen() } }
    }
  }
}

@Composable
fun RecipesScreen() {
  var state by remember { mutableStateOf<UiState>(UiState.Loading) }

  // Load once on first composition
  LaunchedEffect(Unit) {
    state = try {
      val items = ApiClient.recipes.list()
      UiState.Data(items)
    } catch (t: Throwable) {
      UiState.Error(t.message ?: "Network error")
    }
  }

  when (val s = state) {
    UiState.Loading -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
      CircularProgressIndicator()
    }
    is UiState.Error -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
      Text("Error: ${s.message}")
    }
    is UiState.Data -> RecipesList(s.items)
  }
}

@Composable
fun RecipesList(items: List<Recipe>) {
  if (items.isEmpty()) {
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
      Text("No recipes yet")
    }
  } else {
    LazyColumn(
      modifier = Modifier.fillMaxSize().padding(16.dp),
      verticalArrangement = Arrangement.spacedBy(8.dp)
    ) {
      items(items) { r ->
        Card { Text(r.title, modifier = Modifier.padding(16.dp)) }
      }
    }
  }
}

