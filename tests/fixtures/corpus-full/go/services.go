package main

import "time"

type UserService struct {
	db   *Database
	auth *AuthManager
}

func NewUserService(db *Database, auth *AuthManager) *UserService {
	return &UserService{db: db, auth: auth}
}

func (s *UserService) Register(username, email, password string) *User {
	user := &User{
		ID:        int(time.Now().UnixNano()),
		Username:  username,
		Email:     email,
		CreatedAt: time.Now(),
		IsActive:  true,
	}
	s.db.SaveUser(user)
	return user
}

func (s *UserService) GetProfile(token string) (*User, bool) {
	return s.auth.GetUser(token)
}

type ProductService struct {
	db *Database
}

func NewProductService(db *Database) *ProductService {
	return &ProductService{db: db}
}

func (s *ProductService) GetByID(id int) (*Product, bool) {
	return s.db.FindProduct(id)
}

func (s *ProductService) Restock(id, qty int) bool {
	p, ok := s.db.FindProduct(id)
	if !ok {
		return false
	}
	p.Stock += qty
	s.db.SaveProduct(p)
	return true
}

type OrderService struct {
	db         *Database
	productSvc *ProductService
}

func NewOrderService(db *Database, productSvc *ProductService) *OrderService {
	return &OrderService{db: db, productSvc: productSvc}
}

func (s *OrderService) Create(userID int) *Order {
	order := &Order{
		ID:     int(time.Now().UnixNano()),
		UserID: userID,
		Status: "pending",
	}
	s.db.SaveOrder(order)
	return order
}
