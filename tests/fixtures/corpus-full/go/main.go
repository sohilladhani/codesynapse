package main

import (
	"fmt"
	"os"
)

func main() {
	db := NewDatabase()
	if err := db.Connect(); err != nil {
		fmt.Fprintln(os.Stderr, "db connect error:", err)
		os.Exit(1)
	}
	defer db.Disconnect()

	secretKey := os.Getenv("SECRET_KEY")
	if secretKey == "" {
		secretKey = "dev-secret"
	}

	auth := NewAuthManager(db, secretKey)
	userSvc := NewUserService(db, auth)
	productSvc := NewProductService(db)
	orderSvc := NewOrderService(db, productSvc)

	_ = userSvc
	_ = orderSvc

	fmt.Println("Server started")
}
