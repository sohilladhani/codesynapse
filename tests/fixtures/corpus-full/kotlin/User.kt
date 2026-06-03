package com.example.app

import java.time.Instant

data class User(
    val id: Long,
    val username: String,
    val email: String,
    val createdAt: Instant = Instant.now(),
    var isActive: Boolean = true
) {
    fun displayName(): String = username
    fun deactivate() { isActive = false }
}

data class Product(
    val id: Long,
    val name: String,
    val price: Double,
    var stock: Int = 0,
    val category: String? = null
) {
    fun isAvailable(): Boolean = stock > 0
    fun applyDiscount(pct: Double): Double = price * (1 - pct / 100)
}

data class OrderItem(
    val productId: Long,
    val quantity: Int,
    val price: Double
)

data class Order(
    val id: Long,
    val userId: Long,
    val items: MutableList<OrderItem> = mutableListOf(),
    var total: Double = 0.0,
    var status: String = "pending"
) {
    fun addItem(item: OrderItem) {
        items.add(item)
        total += item.price * item.quantity
    }
}
