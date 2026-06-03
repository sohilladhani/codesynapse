package com.example.app

class Database {
    private val store = mutableMapOf<String, Any>()

    fun saveUser(user: User) { store["user:${user.id}"] = user }
    fun findUser(id: Long): User? = store["user:$id"] as? User
    fun saveProduct(product: Product) { store["product:${product.id}"] = product }
    fun findProduct(id: Long): Product? = store["product:$id"] as? Product
}

class UserService(private val db: Database) {
    fun register(username: String, email: String): User {
        val user = User(id = System.currentTimeMillis(), username = username, email = email)
        db.saveUser(user)
        return user
    }

    fun findById(id: Long): User? = db.findUser(id)

    fun deactivate(id: Long): Boolean {
        val user = db.findUser(id) ?: return false
        user.deactivate()
        db.saveUser(user)
        return true
    }
}

class ProductService(private val db: Database) {
    fun getById(id: Long): Product? = db.findProduct(id)

    fun restock(id: Long, qty: Int): Boolean {
        val product = db.findProduct(id) ?: return false
        product.stock += qty
        db.saveProduct(product)
        return true
    }
}

fun main() {
    val db = Database()
    val userService = UserService(db)
    val user = userService.register("alice", "alice@example.com")
    println("Registered: ${user.displayName()}")
}
