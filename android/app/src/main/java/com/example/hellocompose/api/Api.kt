package com.example.hellocompose.api

import com.example.hellocompose.BuildConfig
import com.google.gson.annotations.SerializedName
import okhttp3.OkHttpClient
import okhttp3.logging.HttpLoggingInterceptor
import retrofit2.Retrofit
import retrofit2.converter.gson.GsonConverterFactory
import retrofit2.http.GET

// Match your Axum response: [{ "id": 1, "title": "..." }, ...]
data class Recipe(
  @SerializedName("id") val id: Long,
  @SerializedName("title") val title: String
)

interface RecipesService {
  @GET("recipes")
  suspend fun list(): List<Recipe>
}

object ApiClient {
  private val okHttp = OkHttpClient.Builder()
    .addInterceptor(
      HttpLoggingInterceptor().apply {
        // BASIC/HEADERS/BODY — BODY is verbose but handy during dev
        level = HttpLoggingInterceptor.Level.BASIC
      }
    )
    .build()

  private val retrofit = Retrofit.Builder()
    .baseUrl(BuildConfig.BASE_URL.ensureTrailingSlash())
    .addConverterFactory(GsonConverterFactory.create())
    .client(okHttp)
    .build()

  val recipes: RecipesService = retrofit.create(RecipesService::class.java)
}

private fun String.ensureTrailingSlash(): String =
  if (endsWith("/")) this else "$this/"

